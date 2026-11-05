PREFIX ?= $(HOME)/Applications
BINDIR ?= $(HOME)/.local/bin

APP := dist/Player.app
INSTALLED_APP := $(DESTDIR)$(PREFIX)/Player.app
INSTALLED_PLAYER := $(DESTDIR)$(BINDIR)/player

.PHONY: all release linux test install uninstall

ifeq ($(shell uname -s),Darwin)
all: release

install: release
	mkdir -p "$(DESTDIR)$(PREFIX)"
	rm -rf "$(INSTALLED_APP)"
	ditto "$(APP)" "$(INSTALLED_APP)"
	printf 'installed %s\n' "$(INSTALLED_APP)"

uninstall:
	rm -rf "$(INSTALLED_APP)"
else
all: linux

install: linux
	mkdir -p "$(DESTDIR)$(BINDIR)"
	install -m 755 dist/player "$(INSTALLED_PLAYER)"
	printf 'installed %s\n' "$(INSTALLED_PLAYER)"

uninstall:
	rm -f "$(INSTALLED_PLAYER)"
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
	scripts/linux-container.sh scripts/release-linux.sh
