# Copilot instructions for smart-keyboard

## Configuration documentation rule

Whenever a change adds, removes, or modifies a configuration setting
(anything in `config.toml`, the `[input.*]`, `[mouse]`, `[navigate]`,
`[output.*]`, `[ui.*]`, or `[keymap.*]` sections), **you must also update
`CONFIGURATION.md`** to keep the documentation in sync.

This includes:
- Adding or removing a key in any `config.rs` struct that maps to a TOML field.
- Changing the default value or allowed values of an existing setting.
- Adding a new TOML section.
- Changing the behaviour of the pop-up menu or the configuration editor.

The `CONFIGURATION.md` file documents every TOML section, every key with its
default value and description, the pop-up menu items, and the built-in
configuration editor.
