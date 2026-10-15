#!/bin/sh
# Turns a fresh copy of the RustyHusky template into a new game:
#   ./scripts/new-game.sh "My Game"
# Sets the name in src/config.rs and Cargo.toml, starts a new CHANGELOG.md
# and docs/GAME_RULES.md, and resets the version to 0.1.0. Run it once, in a
# repository created with "Use this template"; see docs/NEW_GAME.md.
set -eu
cd "$(dirname "$0")/.."
name=${1:?usage: $0 "Game Name"}
# "My Game" -> id "my-game", prefix "MY_GAME"
id=$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]\{1,\}/-/g; s/^-//; s/-$//')
prefix=$(printf '%s' "$id" | tr '[:lower:]-' '[:upper:]_')
[ -n "$id" ] || { echo "the name needs at least one letter or digit" >&2; exit 1; }
grep -q '^pub const APP_ID: &str = "rustyhusky";' src/config.rs \
    || { echo "src/config.rs is not the template's any more; nothing changed" >&2; exit 1; }

sed -i.bak \
    -e "s|^pub const APP_NAME: &str = \".*\";|pub const APP_NAME: \&str = \"$name\";|" \
    -e "s|^pub const APP_ID: &str = \".*\";|pub const APP_ID: \&str = \"$id\";|" \
    -e "s|^pub const ENV_PREFIX: &str = \".*\";|pub const ENV_PREFIX: \&str = \"$prefix\";|" \
    src/config.rs
sed -i.bak \
    -e "s|^name = \"rustyhusky\"|name = \"$id\"|" \
    -e "s|^version = \".*\"|version = \"0.1.0\"|" \
    -e "s|^description = \".*\"|description = \"$name, built on the RustyHusky template\"|" \
    -e "/^repository = /d" \
    Cargo.toml
rm -f src/config.rs.bak Cargo.toml.bak

cat > CHANGELOG.md <<EOT
# Changelog

All notable changes to $name are documented here.

## [Unreleased]

### Added

- Started from the RustyHusky template 0.2.0.
EOT

cat > docs/GAME_RULES.md <<EOT
# Game rules

*Describe $name here: goal, controls, scoring, how a level ends, levels and
difficulty, and any numbers a player or developer needs to know.*

Framework behaviour every game gets (pause with Esc/P, results, recording)
is described in the README.
EOT

cargo build --quiet
echo "Done: \"$name\" (id $id, environment prefix ${prefix}_)."
echo "Next: write your game module and set ActiveGame; see docs/NEW_GAME.md."
