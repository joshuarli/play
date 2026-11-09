BINDIR ?= $(HOME)/.local/bin

APP := dist/Player.app

.PHONY: all release linux test install uninstall

ifeq ($(shell uname -s),Darwin)
PLAYER_HOME ?= $(HOME)/Applications/Player
INSTALLED_HOME := $(DESTDIR)$(PLAYER_HOME)

all: release

install: release
	rm -rf "$(INSTALLED_HOME)"
	mkdir -p "$(INSTALLED_HOME)"
	ditto "$(APP)" "$(INSTALLED_HOME)/Player.app"
	cp -RP dist/lib/ "$(INSTALLED_HOME)/lib"
	printf 'installed %s\n' "$(INSTALLED_HOME)/Player.app"

uninstall:
	rm -rf "$(INSTALLED_HOME)"
else
PLAYER_HOME ?= $(HOME)/.local/opt/player
INSTALLED_HOME := $(DESTDIR)$(PLAYER_HOME)
INSTALLED_PLAYER := $(DESTDIR)$(BINDIR)/player

all: linux

install: linux
	rm -rf "$(INSTALLED_HOME)"
	mkdir -p "$(INSTALLED_HOME)/bin" "$(DESTDIR)$(BINDIR)"
	install -m 755 dist/bin/player "$(INSTALLED_HOME)/bin/player"
	cp -RL dist/lib/ "$(INSTALLED_HOME)/lib"
	ln -sfn "$(PLAYER_HOME)/bin/player" "$(INSTALLED_PLAYER)"
	printf 'installed %s\n' "$(INSTALLED_PLAYER)"

uninstall:
	rm -f "$(INSTALLED_PLAYER)"
	rm -rf "$(INSTALLED_HOME)"
endif

test:
ifeq ($(shell uname -s),Darwin)
	@mkdir -p .work/tmp .work/fixtures .work/deps
	@if git rev-parse --git-dir >/dev/null 2>&1; then \
		d=$$(git rev-parse --git-dir)/info; mkdir -p "$$d"; \
		grep -qxF '.work/' "$$d/exclude" 2>/dev/null || echo '.work/' >> "$$d/exclude"; \
	fi
	@command -v brew >/dev/null 2>&1 || { echo 'error: Homebrew required for macOS test deps' >&2; exit 1; }
	@scripts/build-deps.sh
	@FFMPEG_BIN_DIR=""; \
	if command -v ffmpeg >/dev/null 2>&1 && ffmpeg -hide_banner -encoders 2>/dev/null | grep -q 'libx264'; then \
		:; \
	elif [ -x /opt/homebrew/Cellar/ffmpeg/9.0.2_1/bin/ffmpeg ]; then \
		FFMPEG_BIN_DIR=/opt/homebrew/Cellar/ffmpeg/9.0.2_1/bin; \
	elif command -v brew >/dev/null 2>&1 && [ -x "$$(brew --prefix 2>/dev/null)/opt/ffmpeg/bin/ffmpeg" ]; then \
		FFMPEG_BIN_DIR="$$(brew --prefix)/opt/ffmpeg/bin"; \
	fi; \
	if [ -n "$$FFMPEG_BIN_DIR" ]; then export PATH="$$FFMPEG_BIN_DIR:$$PATH"; fi; \
	scripts/build.sh && tests/run.sh suite
else
	scripts/linux-container.sh tests/run.sh suite
endif

release:
	scripts/release.sh

linux:
	scripts/linux-container.sh scripts/build-deps-linux.sh
	scripts/linux-container.sh scripts/build-rust-fontconfig-linux.sh
	scripts/linux-container.sh scripts/release-linux.sh
