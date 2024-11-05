SHELL = /bin/bash
OUTPUT_DIR = $$(pwd)/bin
ID = `cat config.yml | head -n 1 | cut -d \" -f 2`
NAME = `cat config.yml | head -n 2 | cut -d \" -f 2 | tail -n 1`
STATIC_DIR = /assets/$(ID)
BIN_DIR = /$(NAME)
BUILD_TYPE = debug
TARGET_DIR = $$(pwd)/target/$(BUILD_TYPE)
PLUGIN_SUFFIX =

ifeq ($(OS),Windows_NT)
    PLUGIN_SUFFIX = .dll
else
    UNAME_S := $(shell uname -s)
    ifeq ($(UNAME_S),Linux)
        PLUGIN_SUFFIX = .so
    endif
    ifeq ($(UNAME_S),Darwin)
        PLUGIN_SUFFIX = .dylib
    endif
endif

.PHONY: check static output clean help

## check: Check code and style.
check:
	@cargo clippy -- -D clippy::all
	@cargo fmt --all -- --check

## static: Build static files.
static:
	@rm -rf $(OUTPUT_DIR)$(STATIC_DIR) && mkdir -p $(OUTPUT_DIR)$(STATIC_DIR)
	@cd frontend && yarn && yarn build
	@cp -r frontend/dist/. $(OUTPUT_DIR)$(STATIC_DIR)

## output: Copy build files for production.
output:
	@rm -rf $(OUTPUT_DIR)$(BIN_DIR) && mkdir -p $(OUTPUT_DIR)$(BIN_DIR)
	@cp $(TARGET_DIR)/*$(NAME)$(PLUGIN_SUFFIX) $(OUTPUT_DIR)$(BIN_DIR)
	@cp config.yml $(OUTPUT_DIR)$(BIN_DIR)

## clean: Clean all build files.
clean:
	@rm -rf $(OUTPUT_DIR)
	@cd frontend && rm -rf dist && rm -rf node_modules
	@cargo clean

## help: Show this help.
help: Makefile
	@echo Usage: make [command]
	@sed -n 's/^##//p' $< | column -t -s ':' |  sed -e 's/^/ /'
