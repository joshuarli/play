PREFIX ?= $(HOME)/Applications
BINDIR ?= $(HOME)/.local/bin

APP := dist/Player.app
INSTALLED_APP := $(DESTDIR)$(PREFIX)/Player.app
INSTALLED_PLAYER := $(DESTDIR)$(BINDIR)/player

.PHONY: all release linux install uninstall

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

release:
	scripts/release.sh

linux:
	scripts/linux-container.sh scripts/build-deps-linux.sh
	scripts/linux-container.sh scripts/release-linux.sh
