CC = gcc
CFLAGS = -Wall -Werror -g
LDFLAGS = -L. -lrustfontconfig
RUST_FLAGS = --release --features ffi
INCLUDE_DIR = include

.PHONY: all clean

all: librustfontconfig.a librustfontconfig.so example

librustfontconfig.a librustfontconfig.so: ffi.rs
	cargo build $(RUST_FLAGS)
	cp target/release/librustfontconfig.a .
	cp target/release/librustfontconfig.so .

example: ffi/example.c include/fontconfig-c.h librustfontconfig.a
	$(CC) $(CFLAGS) -I$(INCLUDE_DIR) -o $@ $< $(LDFLAGS)

include/fontconfig-c.h:
	mkdir -p include
	cp ffi/fontconfig-c.h include/

clean:
	rm -f example
	rm -f librustfontconfig.a librustfontconfig.so
	cargo clean

# Windows-specific targets
.PHONY: win

win: rustfontconfig.lib rustfontconfig.dll example.exe

rustfontconfig.lib rustfontconfig.dll:
	cargo build $(RUST_FLAGS)
	copy target\release\rustfontconfig.lib .
	copy target\release\rustfontconfig.dll .

example.exe: ffi/example.c include/fontconfig-c.h rustfontconfig.lib
	cl.exe /W4 /EHsc /Fe:example.exe ffi/example.c /I$(INCLUDE_DIR) /link /LIBPATH:. rustfontconfig.lib

# macOS-specific targets
.PHONY: mac

mac: librustfontconfig.a librustfontconfig.dylib example

librustfontconfig.dylib:
	cargo build $(RUST_FLAGS)
	cp target/release/librustfontconfig.dylib .