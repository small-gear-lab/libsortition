# Builds the example as C and as C++ against the shared library that `cargo build --release` makes.
ROOT    := ../..
INCLUDE := -I$(ROOT)/core/include
LINK    := -L$(ROOT)/target/release -lsortition -Wl,-rpath,$(abspath $(ROOT)/target/release)

all: demo demo-cpp

demo: demo.c
	$(CC) -std=c99 -Wall -Wextra -pedantic $(INCLUDE) $< $(LINK) -o $@

demo-cpp: demo.c
	$(CXX) -x c++ -std=c++17 -Wall -Wextra -pedantic $(INCLUDE) $< $(LINK) -o $@

run: all
	./demo
	./demo-cpp

# What CI runs: both programs must print the winners that every other binding gives.
check: all
	test "$$(./demo | sed -n 1p)" = "dave bob"
	test "$$(./demo-cpp | sed -n 1p)" = "dave bob"

clean:
	rm -f demo demo-cpp

.PHONY: all run check clean
