# Enhanced Clipboard

[简体中文](./README.zh-CN.md)

A small Windows clipboard manager built mainly for my own use, then open-sourced because it might be useful to someone else too.

The goal is simple: make clipboard history a bit more practical than the default Windows experience, without turning it into an oversized productivity app.

It is intentionally focused, a little opinionated, and developed at my own pace.

---

## What this is

Enhanced Clipboard is a desktop clipboard manager built with **Tauri**, **Rust**, **Vue 3**, and **TypeScript**.

It is primarily a **personal-use project**. I use it myself, keep improving the parts that matter to me, and publish it here in case others find it useful as well.

That also means development is driven by real usage, not by trying to satisfy every possible use case.

---

## Why I made it

Windows clipboard history is useful, but for my own workflow it feels a bit too limited.

I wanted a tool that makes it easier to:

- browse recent clipboard items
- search for something copied earlier
- filter by date
- pin frequently used entries
- quickly copy text or images back to the clipboard

So I made one.

---

## Features

- Clipboard history for text and images
- Keyword search
- Entry-type filtering via slash command menu
- Date filtering
- Pinned entries
- Copy-back for saved items
- Local storage
- A small set of practical settings

Nothing especially flashy — just features that are actually useful in day-to-day use.

---

## Tech Stack

- **Tauri v2**
- **Rust**
- **Vue 3**
- **TypeScript**
- **Pinia**
- **SQLite / SQLCipher**

---

## Scope

This project is meant to stay relatively small and focused.

It is **not** trying to be:

- a cloud sync service
- a team productivity tool
- a note-taking app
- a “do everything” clipboard platform

It is just a local desktop clipboard manager with a cleaner and more practical workflow.

---

## Project Status

This project is usable and actively used by me, but it is still a personal project first.

A few expectations that are worth stating clearly:

- updates may be irregular
- bug fixes are not guaranteed to be immediate
- feature additions depend mostly on whether they fit the project and whether I personally need them
- the project is unlikely to expand just for the sake of broader appeal

Issues and pull requests are welcome, but the repository is not run like a fast-moving community project.

---

## Development

### Clone the repository

```bash
git clone https://github.com/kuonji-arisu/enhanced-clipboard.git
cd enhanced-clipboard
````

### Install dependencies

```bash
pnpm install
```

### Run in development mode

```bash
pnpm tauri dev
```

### Build

```bash
pnpm tauri build
```

---

## Screenshots

Not properly organized yet.
I may add some later.

---

## Contributing

You are welcome to open an issue or submit a pull request.

That said, this repository is still mainly maintained according to my own needs and priorities, so responses and merges may take time.

---

## License

MIT

---

## Notes

This project was not created to be a large polished product.
It was created because I wanted a clipboard tool that felt better for everyday use.

If it happens to work well for you too, that is a nice bonus.
