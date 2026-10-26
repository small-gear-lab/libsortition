package sortition

import (
	"context"
	"crypto/rand"
	_ "embed"
	"fmt"
	"sync"

	"github.com/tetratelabs/wazero"
	"github.com/tetratelabs/wazero/api"
)

// The core, compiled to WebAssembly (bindings/wasm/build.sh). The library has no cgo dependency: it
// runs the module with wazero. Update the copy with `go generate` after the core changes.
//
//go:generate sh -c "cd ../.. && bindings/wasm/build.sh && cp bindings/wasm/sortition.wasm bindings/go/sortition.wasm"
//go:embed sortition.wasm
var wasmBinary []byte

// The module instance is shared by the whole process and is not safe for concurrent use,
// so every call into it holds lock.
var (
	lock     sync.Mutex
	once     sync.Once
	instance *host
)

type host struct {
	ctx context.Context
	mod api.Module
	mem api.Memory
	fns map[string]api.Function
}

func get() *host {
	once.Do(func() {
		ctx := context.Background()
		rt := wazero.NewRuntime(ctx)
		// wasm32-unknown-unknown has no source of randomness: the host provides it.
		_, err := rt.NewHostModuleBuilder("env").
			NewFunctionBuilder().
			WithGoModuleFunction(api.GoModuleFunc(func(_ context.Context, m api.Module, stack []uint64) {
				ptr, n := api.DecodeU32(stack[0]), api.DecodeU32(stack[1])
				buf, ok := m.Memory().Read(ptr, n)
				if !ok {
					panic("sortition: random_fill out of range")
				}
				if _, err := rand.Read(buf); err != nil {
					panic(fmt.Sprintf("sortition: no randomness: %v", err))
				}
			}), []api.ValueType{api.ValueTypeI32, api.ValueTypeI32}, nil).
			Export("sortition_random_fill").
			Instantiate(ctx)
		if err != nil {
			panic(fmt.Sprintf("sortition: %v", err))
		}
		mod, err := rt.InstantiateWithConfig(ctx, wasmBinary, wazero.NewModuleConfig().WithName("sortition"))
		if err != nil {
			panic(fmt.Sprintf("sortition: %v", err))
		}
		instance = &host{ctx: ctx, mod: mod, mem: mod.Memory(), fns: map[string]api.Function{}}
	})
	return instance
}

// call runs an exported function and returns its raw result (0 if it returns nothing).
func (h *host) call(name string, args ...uint64) uint64 {
	fn, ok := h.fns[name]
	if !ok {
		fn = h.mod.ExportedFunction(name)
		if fn == nil {
			panic("sortition: missing export " + name)
		}
		h.fns[name] = fn
	}
	out, err := fn.Call(h.ctx, args...)
	if err != nil {
		panic(fmt.Sprintf("sortition: %s trapped: %v", name, err))
	}
	if len(out) == 0 {
		return 0
	}
	return out[0]
}

// frame owns the buffers of one call in the module's linear memory.
type frame struct {
	h      *host
	allocs [][2]uint32
}

// begin takes the lock and returns a frame; end releases both.
func begin() *frame {
	lock.Lock()
	return &frame{h: get()}
}

func (f *frame) end() {
	for _, a := range f.allocs {
		f.h.call("sortition_dealloc", api.EncodeU32(a[0]), api.EncodeU32(a[1]))
	}
	f.allocs = nil
	lock.Unlock()
}

func (f *frame) alloc(n uint32) uint32 {
	p := api.DecodeU32(f.h.call("sortition_alloc", api.EncodeU32(n)))
	f.allocs = append(f.allocs, [2]uint32{p, n})
	return p
}

// put copies b to the module, followed by extra zero bytes (1 for a NUL terminator).
func (f *frame) put(b []byte, extra uint32) uint32 {
	p := f.alloc(uint32(len(b)) + extra)
	if len(b) > 0 && !f.h.mem.Write(p, b) {
		panic("sortition: write out of range")
	}
	if extra > 0 && !f.h.mem.Write(p+uint32(len(b)), make([]byte, extra)) {
		panic("sortition: write out of range")
	}
	return p
}

func (f *frame) putSeed(seed *uint64) uint32 {
	if seed == nil {
		return 0
	}
	p := f.alloc(8)
	if !f.h.mem.WriteUint64Le(p, *seed) {
		panic("sortition: write out of range")
	}
	return p
}

// putItems writes the candidates as one buffer and an array of lengths (a wasm32 usize is 4 bytes).
func (f *frame) putItems(chunks [][]byte) (data, lens, count uint32) {
	total := 0
	for _, c := range chunks {
		total += len(c)
	}
	buf := make([]byte, 0, total)
	lenBytes := make([]byte, 0, 4*len(chunks))
	for _, c := range chunks {
		buf = append(buf, c...)
		lenBytes = append(lenBytes, byte(len(c)), byte(len(c)>>8), byte(len(c)>>16), byte(len(c)>>24))
	}
	return f.put(buf, 0), f.put(lenBytes, 0), uint32(len(chunks))
}

func (f *frame) bytes(p, n uint32) []byte {
	b, ok := f.h.mem.Read(p, n)
	if !ok {
		panic("sortition: read out of range")
	}
	return append([]byte(nil), b...)
}

func (f *frame) u32(p uint32) uint32 {
	v, ok := f.h.mem.ReadUint32Le(p)
	if !ok {
		panic("sortition: read out of range")
	}
	return v
}

func (f *frame) u32s(p, n uint32) []uint32 {
	out := make([]uint32, n)
	for i := range out {
		out[i] = f.u32(p + 4*uint32(i))
	}
	return out
}

// cString reads a NUL-terminated string.
func (f *frame) cString(p uint32) string {
	mem := f.h.mem
	var out []byte
	for {
		b, ok := mem.Read(p+uint32(len(out)), 1)
		if !ok {
			panic("sortition: read out of range")
		}
		if b[0] == 0 {
			return string(out)
		}
		out = append(out, b[0])
	}
}

// takeString reads a string that the library allocated with sortition_free semantics, and frees it.
func (f *frame) takeString(p uint32) string {
	defer f.h.call("sortition_free", api.EncodeU32(p))
	return f.cString(p)
}

// lastError is the detailed message of the last failure (empty if there is none).
func (f *frame) lastError() string {
	p := api.DecodeU32(f.h.call("sortition_last_error"))
	if p == 0 {
		return ""
	}
	return f.cString(p)
}
