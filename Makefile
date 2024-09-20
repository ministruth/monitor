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

.PHONY: static output

static:
	@rm -rf $(OUTPUT_DIR)$(STATIC_DIR) && mkdir -p $(OUTPUT_DIR)$(STATIC_DIR)
	@cd frontend && yarn && yarn build
	@cp -r frontend/dist/. $(OUTPUT_DIR)$(STATIC_DIR)

output:
	@rm -rf $(OUTPUT_DIR)$(BIN_DIR) && mkdir -p $(OUTPUT_DIR)$(BIN_DIR)
	@cp $(TARGET_DIR)/*$(NAME)$(PLUGIN_SUFFIX) $(OUTPUT_DIR)$(BIN_DIR)
	@cp config.yml $(OUTPUT_DIR)$(BIN_DIR)
