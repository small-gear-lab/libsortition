# frozen_string_literal: true

require "fiddle"
require "fiddle/import"
require "json"

# libsortition の Ruby バインディング（標準ライブラリの Fiddle のみ）
module Sortition
  extend Fiddle::Importer

  ERRORS = {
    -1 => "invalid argument (k > n?)",
    -2 => "OS random source failed",
    -3 => "text is not valid UTF-8",
    -4 => "parse error",
    -5 => "path not found or not an array",
    -6 => "unsupported format",
    -7 => "internal error",
    -8 => "invalid session state"
  }.freeze

  FORMATS = { json: 0, yaml: 1, toml: 2 }.freeze

  # コアが返したエラー。code に SORTITION_ERR_* の値が入る。
  class Error < ArgumentError
    attr_reader :code

    def initialize(code, detail = nil)
      @code = code
      super(detail || ERRORS.fetch(code, "error #{code}"))
    end
  end

  def self.lib_name
    case RbConfig::CONFIG["host_os"]
    when /darwin/ then "libsortition.dylib"
    when /mswin|mingw/ then "sortition.dll"
    else "libsortition.so"
    end
  end

  def self.find_lib
    [
      ENV["LIBSORTITION_PATH"],
      File.expand_path(lib_name, __dir__),
      File.expand_path("../../../target/release/#{lib_name}", __dir__)
    ].compact.find { |p| File.exist?(p) } or raise "libsortition not found; set LIBSORTITION_PATH"
  end

  dlload find_lib
  extern "int sortition_draw(unsigned int, unsigned int, void*, void*)"
  extern "long sortition_draw_text(void*, unsigned long, unsigned int, void*, void*, unsigned long)"
  extern "int sortition_draw_data(int, void*, unsigned long, void*, unsigned int, void*, void*)"
  extern "int sortition_session_new(unsigned int, void*, void*)"
  extern "int sortition_session_draw(void*, unsigned int, void*)"
  extern "unsigned int sortition_session_remaining(void*)"
  extern "unsigned int sortition_session_drawn_count(void*)"
  extern "int sortition_session_save(void*, void*)"
  extern "int sortition_session_load(void*, unsigned long, void*)"
  extern "void sortition_session_free(void*)"
  extern "int sortition_item_ids(void*, void*, unsigned long, void*)"
  extern "int sortition_item_fingerprint(void*, void*, unsigned long, void*)"
  extern "int sortition_items_draw(void*, void*, unsigned long, unsigned int, void*, void*, void*)"
  extern "int sortition_item_session_new(void*, void*, unsigned long, void*, void*)"
  extern "int sortition_item_session_draw(void*, unsigned int, void*, void*)"
  extern "unsigned int sortition_item_session_remaining(void*)"
  extern "unsigned int sortition_item_session_drawn_count(void*)"
  extern "int sortition_item_session_save(void*, void*)"
  extern "int sortition_item_session_load(void*, unsigned long, void*, void*, unsigned long, void*)"
  extern "void sortition_item_session_free(void*)"
  extern "void sortition_free(void*)"
  extern "const char* sortition_last_error()"
  extern "const char* sortition_version()"

  # 0...n から k 個を重複なしで抽選して Array で返す。seed 指定で再現可能。
  def self.draw(n, k, seed: nil)
    out = Fiddle::Pointer.malloc(4 * k, Fiddle::RUBY_FREE)
    seed_ptr = seed && Fiddle::Pointer[[seed].pack("Q")]
    rc = sortition_draw(n, k, seed_ptr, out)
    raise Error.new(rc) unless rc.zero?

    out[0, 4 * k].unpack("L#{k}")
  end

  # 1 行 1 候補のテキストから k 個を重複なしで抽選し、選ばれた候補の Array<String> を返す。
  # 各行は前後の空白を除き、空行は候補に含めない。同じ文字列の行は別の候補として数える。
  def self.draw_text(text, k, seed: nil)
    data = text.encode("UTF-8").b
    out = Fiddle::Pointer.malloc(data.bytesize + 1, Fiddle::RUBY_FREE)
    seed_ptr = seed && Fiddle::Pointer[[seed].pack("Q")]
    n = sortition_draw_text(data, data.bytesize, k, seed_ptr, out, data.bytesize + 1)
    raise Error.new(n) if n.negative?

    n.zero? ? [] : out[0, n].force_encoding("UTF-8").split("\n")
  end

  # JSON / YAML / TOML のテキストにある配列から k 個を重複なしで抽選し、選ばれた要素を返す。
  #
  # format は :json | :yaml | :toml。path は配列の場所（"a.b.0" のドット区切り）で、
  # 空ならルートが配列であること。TOML はルートがテーブルなので path が必要。
  # 要素は JSON に正規化される（TOML の日時は文字列。YAML のエイリアス・独自タグ・
  # 複数ドキュメントは Sortition::Error）。
  def self.draw_data(text, k, format:, path: "", seed: nil)
    fmt = FORMATS.fetch(format.to_sym) { raise ArgumentError, "format must be one of #{FORMATS.keys}" }
    data = text.encode("UTF-8").b
    seed_ptr = seed && Fiddle::Pointer[[seed].pack("Q")]
    out = Fiddle::Pointer.malloc(Fiddle::SIZEOF_VOIDP, Fiddle::RUBY_FREE)
    out[0, Fiddle::SIZEOF_VOIDP] = "\0" * Fiddle::SIZEOF_VOIDP
    rc = sortition_draw_data(fmt, data, data.bytesize, "#{path}\0".b, k, seed_ptr, out)
    raise Error.new(rc, sortition_last_error.to_s.dup.force_encoding("UTF-8")) unless rc.zero?

    addr = out[0, Fiddle::SIZEOF_VOIDP].unpack1("J")
    begin
      JSON.parse(Fiddle::Pointer.new(addr).to_s.force_encoding("UTF-8"))
    ensure
      sortition_free(Fiddle::Pointer.new(addr))
    end
  end

  # ポインタ 1 個分の out 領域を確保して 0 で埋める
  def self.out_pointer
    out = Fiddle::Pointer.malloc(Fiddle::SIZEOF_VOIDP, Fiddle::RUBY_FREE)
    out[0, Fiddle::SIZEOF_VOIDP] = "\0" * Fiddle::SIZEOF_VOIDP
    out
  end

  def self.read_address(out) = out[0, Fiddle::SIZEOF_VOIDP].unpack1("J")

  # 抽選セッション。0...n から呼ぶたびに続きから非復元抽出する。
  #
  # #save で状態を JSON 文字列にでき、Session.load で復元して続きから引ける。
  # seed 指定ありなら、何回に分けて引いても Sortition.draw(n, k1 + k2 + ..., seed:) と同じ並びになる。
  # 1 つの Session を複数スレッドから同時に使わないこと。
  class Session
    def initialize(n, seed: nil)
      out = Sortition.out_pointer
      seed_ptr = seed && Fiddle::Pointer[[seed].pack("Q")]
      rc = Sortition.sortition_session_new(n, seed_ptr, out)
      raise Error.new(rc) unless rc.zero?

      adopt(Sortition.read_address(out))
    end

    # #save の出力から復元する。不正な状態は Sortition::Error（code -8）。
    def self.load(state)
      data = state.encode("UTF-8").b
      out = Sortition.out_pointer
      rc = Sortition.sortition_session_load(data, data.bytesize, out)
      unless rc.zero?
        raise Error.new(rc, Sortition.sortition_last_error.to_s.dup.force_encoding("UTF-8"))
      end

      allocate.tap { |s| s.send(:adopt, Sortition.read_address(out)) }
    end

    # 続きから k 個引いて Array で返す。残りを超えると Sortition::Error（状態は変わらない）。
    def draw(k)
      out = Fiddle::Pointer.malloc([4 * k, 1].max, Fiddle::RUBY_FREE)
      rc = Sortition.sortition_session_draw(handle, k, out)
      raise Error.new(rc) unless rc.zero?

      k.zero? ? [] : out[0, 4 * k].unpack("L#{k}")
    end

    # まだ引いていない数
    def remaining = Sortition.sortition_session_remaining(handle)

    # ここまでに引いた数
    def drawn_count = Sortition.sortition_session_drawn_count(handle)

    # 状態を JSON 文字列で返す。引いた結果（"drawn"）も含む。
    def save
      out = Sortition.out_pointer
      rc = Sortition.sortition_session_save(handle, out)
      raise Error.new(rc) unless rc.zero?

      addr = Sortition.read_address(out)
      begin
        Fiddle::Pointer.new(addr).to_s.force_encoding("UTF-8")
      ensure
        Sortition.sortition_free(Fiddle::Pointer.new(addr))
      end
    end

    # コア側のセッションを解放する（以後の利用は IOError）。二重に呼んでもよい。
    def close
      return unless @addr

      ObjectSpace.undefine_finalizer(self)
      Sortition.sortition_session_free(Fiddle::Pointer.new(@addr))
      @addr = nil
    end

    private

    def adopt(addr)
      @addr = addr
      ObjectSpace.define_finalizer(self, self.class.send(:finalizer, addr))
    end

    def handle
      raise IOError, "session is closed" unless @addr

      Fiddle::Pointer.new(@addr)
    end

    # self を捕まえないよう、クラスメソッドで作る
    def self.finalizer(addr)
      proc { Sortition.sortition_session_free(Fiddle::Pointer.new(addr)) }
    end
    private_class_method :finalizer
  end

  # 当選した候補。index は渡した items の中の位置、id は候補の ID（小文字 16 進 64 文字）。
  Pick = Struct.new(:index, :id)

  # 候補（String。バイト列としてそのまま扱う）を、連結バッファと長さ配列（size_t の packed）にする。
  def self.pack_items(items)
    chunks = items.map do |item|
      raise TypeError, "candidate must be a String, not #{item.class}" unless item.is_a?(String)

      item.b
    end
    [chunks.join.b, chunks.map(&:bytesize).pack("J*"), chunks.size]
  end

  def self.read_picks(indexes, ids, k)
    return [] if k.zero?

    idx = indexes[0, 4 * k].unpack("L#{k}")
    raw = ids[0, 32 * k]
    Array.new(k) { |i| Pick.new(idx[i], raw.byteslice(32 * i, 32).unpack1("H*")) }
  end

  # 各候補の ID（小文字 16 進 64 文字）を items と同じ並びで返す。
  #
  # ID は SHA-256(接頭辞 || バイト列 || 出現番号)。同じバイト列が複数あっても、
  # 先頭から数えた出現番号が混ざるので ID は全候補で一意になる。
  def self.item_ids(items)
    data, lens, n = pack_items(items)
    out = Fiddle::Pointer.malloc([32 * n, 1].max, Fiddle::RUBY_FREE)
    rc = sortition_item_ids(data, lens, n, out)
    raise Error.new(rc) unless rc.zero?

    raw = n.zero? ? "" : out[0, 32 * n]
    Array.new(n) { |i| raw.byteslice(32 * i, 32).unpack1("H*") }
  end

  # 候補集合の指紋（小文字 16 進 64 文字）。items の並びに依らない。
  # 抽選前にシードと一緒に公開しておくと、あとから同じ候補集合だったか検証できる。
  def self.item_fingerprint(items)
    data, lens, n = pack_items(items)
    out = Fiddle::Pointer.malloc(32, Fiddle::RUBY_FREE)
    rc = sortition_item_fingerprint(data, lens, n, out)
    raise Error.new(rc) unless rc.zero?

    out[0, 32].unpack1("H*")
  end

  # items（String）から k 個を重複なしで抽選し、Array<Pick> で返す。
  #
  # 候補は ID の昇順に並べてから引くので、同じ候補集合とシードなら、items の並びが違っても
  # 同じ候補が当選する（Pick#index は各 items の中の位置）。
  def self.draw_items(items, k, seed: nil)
    data, lens, n = pack_items(items)
    seed_ptr = seed && Fiddle::Pointer[[seed].pack("Q")]
    indexes = Fiddle::Pointer.malloc([4 * k, 1].max, Fiddle::RUBY_FREE)
    ids = Fiddle::Pointer.malloc([32 * k, 1].max, Fiddle::RUBY_FREE)
    rc = sortition_items_draw(data, lens, n, k, seed_ptr, indexes, ids)
    raise Error.new(rc) unless rc.zero?

    read_picks(indexes, ids, k)
  end

  # 候補集合を固定した抽選セッション（保存・復元つき）。
  #
  # #save の状態には引いた候補の ID と候補集合の指紋が入る。ItemSession.load では同じ候補集合
  # （並びは違ってよい）を渡し直し、違えば Sortition::Error（code -8）。復元後の Pick#index は
  # 渡し直した items の中の位置になる。1 つの ItemSession を複数スレッドから同時に使わないこと。
  class ItemSession
    def initialize(items, seed: nil)
      data, lens, n = Sortition.pack_items(items)
      out = Sortition.out_pointer
      seed_ptr = seed && Fiddle::Pointer[[seed].pack("Q")]
      rc = Sortition.sortition_item_session_new(data, lens, n, seed_ptr, out)
      raise Error.new(rc) unless rc.zero?

      adopt(Sortition.read_address(out))
    end

    # #save の出力と候補から復元する。
    def self.load(state, items)
      raw = state.b
      data, lens, n = Sortition.pack_items(items)
      out = Sortition.out_pointer
      rc = Sortition.sortition_item_session_load(raw, raw.bytesize, data, lens, n, out)
      unless rc.zero?
        raise Error.new(rc, Sortition.sortition_last_error.to_s.dup.force_encoding("UTF-8"))
      end

      allocate.tap { |s| s.send(:adopt, Sortition.read_address(out)) }
    end

    # 続きから k 個引いて Array<Pick> で返す。残りを超えると Sortition::Error（状態は変わらない）。
    def draw(k)
      indexes = Fiddle::Pointer.malloc([4 * k, 1].max, Fiddle::RUBY_FREE)
      ids = Fiddle::Pointer.malloc([32 * k, 1].max, Fiddle::RUBY_FREE)
      rc = Sortition.sortition_item_session_draw(handle, k, indexes, ids)
      raise Error.new(rc) unless rc.zero?

      Sortition.read_picks(indexes, ids, k)
    end

    def remaining = Sortition.sortition_item_session_remaining(handle)

    def drawn_count = Sortition.sortition_item_session_drawn_count(handle)

    # 状態を JSON 文字列で返す。引いた候補の ID（"drawn"）と候補集合の指紋（"fingerprint"）を含む。
    def save
      out = Sortition.out_pointer
      rc = Sortition.sortition_item_session_save(handle, out)
      raise Error.new(rc) unless rc.zero?

      addr = Sortition.read_address(out)
      begin
        Fiddle::Pointer.new(addr).to_s.force_encoding("UTF-8")
      ensure
        Sortition.sortition_free(Fiddle::Pointer.new(addr))
      end
    end

    # コア側のセッションを解放する（以後の利用は IOError）。二重に呼んでもよい。
    def close
      return unless @addr

      ObjectSpace.undefine_finalizer(self)
      Sortition.sortition_item_session_free(Fiddle::Pointer.new(@addr))
      @addr = nil
    end

    private

    def adopt(addr)
      @addr = addr
      ObjectSpace.define_finalizer(self, self.class.send(:finalizer, addr))
    end

    def handle
      raise IOError, "session is closed" unless @addr

      Fiddle::Pointer.new(@addr)
    end

    def self.finalizer(addr)
      proc { Sortition.sortition_item_session_free(Fiddle::Pointer.new(addr)) }
    end
    private_class_method :finalizer
  end

  def self.version = sortition_version.to_s
end
