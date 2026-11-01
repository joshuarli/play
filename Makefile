PREFIX ?= $(HOME)/Applications

APP := dist/Player.app
INSTALLED_APP := $(DESTDIR)$(PREFIX)/Player.app

.PHONY: all release install uninstall

all: release

release:
	scripts/release.sh

install: release
	mkdir -p "$(DESTDIR)$(PREFIX)"
	rm -rf "$(INSTALLED_APP)"
	ditto "$(APP)" "$(INSTALLED_APP)"
	printf 'installed %s\n' "$(INSTALLED_APP)"

uninstall:
	rm -rf "$(INSTALLED_APP)"
