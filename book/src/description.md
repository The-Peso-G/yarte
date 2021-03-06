# Description

Yarte stands for **Y**et **A**nother **R**ust **T**emplate **E**ngine, 
is the fastest template engine. Uses a Handlebars-like syntax, 
well known and intuitive for most developers. Yarte is an optimized, and easy-to-use 
rust crate, with which developers can create logic around their 
HTML templates using using conditionals, loops, rust code 
and using templates composition with partials.

## Derive attributes
- `src`: template sources
- `path`: path to sources relative to template directory
- `print`: `all`, `ast` or `code` display debug info. Overridden by config file print option.
- `mode`: `text` or `html`
- `ext`: Set file extension
###### `with-actix-web` feature 
- `err`: Set error response body