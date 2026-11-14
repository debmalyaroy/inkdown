# Inkdown build entry point for macOS, Linux and Git Bash on Windows.
# On Windows without `make`, use build.ps1 (same targets).
#
#   make            release build for this machine
#   make test       unit tests
#   make run        run the debug build
#   make package    the package for this operating system, in dist/
#   make help       everything else

CARGO   ?= cargo
NATIVE  := native
VERSION := $(shell sed -n 's/^version *= *"\(.*\)"/\1/p' $(NATIVE)/Cargo.toml | head -1)

ifeq ($(OS),Windows_NT)
  PLATFORM := windows
  EXE := .exe
else
  UNAME := $(shell uname -s)
  ifeq ($(UNAME),Darwin)
    PLATFORM := macos
  else
    PLATFORM := linux
  endif
  EXE :=
endif

BIN := $(NATIVE)/target/release/inkdown$(EXE)

.PHONY: all build release run test check uitest package windows macos linux android msix clean version help

all: release

help:
	@echo "Inkdown $(VERSION) on $(PLATFORM)"
	@echo "  make build      debug build"
	@echo "  make release    optimised build ($(BIN))"
	@echo "  make run        run the debug build"
	@echo "  make test       unit tests"
	@echo "  make check      type-check only"
	@echo "  make uitest     UI regression suite (Windows, needs pwsh)"
	@echo "  make package    package for this OS into dist/"
	@echo "  make windows    dist/Inkdown-Setup.exe and dist/inkdown.exe"
	@echo "  make msix       Microsoft Store package (Windows SDK needed)"
	@echo "  make macos      dist/macos/Inkdown.app and Inkdown.dmg (run on a Mac)"
	@echo "  make linux      dist/linux/inkdown-linux-x86_64.tar.gz"
	@echo "  make android    dist/android/inkdown.apk (needs Docker)"
	@echo "  make clean      remove build output"

version:
	@echo $(VERSION)

build:
	cd $(NATIVE) && $(CARGO) build --locked

release:
	cd $(NATIVE) && $(CARGO) build --release --locked

run:
	cd $(NATIVE) && $(CARGO) run --locked

test:
	cd $(NATIVE) && $(CARGO) test --locked

check:
	cd $(NATIVE) && $(CARGO) check --locked

uitest: release
	pwsh $(NATIVE)/tools/uitest.ps1 -Release

package: $(PLATFORM)

# The installer is the application itself: an exe whose name contains "setup" installs.
windows: release
	mkdir -p dist
	cp $(BIN) dist/inkdown.exe
	cp $(BIN) dist/Inkdown-Setup.exe

msix: release
	pwsh packaging/windows/make-msix.ps1 -Exe $(BIN) -Version $(VERSION)

macos:
	sh packaging/macos/build.sh

linux: release
	sh packaging/linux/make-tarball.sh

android:
	docker build -f packaging/android/Dockerfile -t inkdown-android .
	mkdir -p dist/android
	docker run --rm -v "$(CURDIR)/dist/android:/out" inkdown-android

clean:
	cd $(NATIVE) && $(CARGO) clean
	rm -rf dist
