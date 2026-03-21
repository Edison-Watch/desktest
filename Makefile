# Current version from Cargo.toml
CURRENT_VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')

.PHONY: install_cli bump_version

install_cli:
	cargo install --path . --locked

## bump_version: Bump the version in Cargo.toml and create a git tag.
##   Usage: make bump_version VERSION=0.3.0
bump_version:
ifndef VERSION
	$(error VERSION is required. Usage: make bump_version VERSION=0.3.0)
endif
	@echo "Bumping version: $(CURRENT_VERSION) -> $(VERSION)"
	@sed -i.bak 's/^version = "$(CURRENT_VERSION)"/version = "$(VERSION)"/' Cargo.toml && rm -f Cargo.toml.bak
	@cargo check --quiet || (echo "cargo check failed — version bump aborted"; exit 1)
	@git add Cargo.toml Cargo.lock
	@git commit -m "chore: bump version to $(VERSION)"
	@git tag "v$(VERSION)"
	@echo ""
	@echo "Version bumped to $(VERSION) and tagged v$(VERSION)."
	@echo "Run 'git push && git push --tags' to trigger the release."
