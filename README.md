# rumdl-lib

The markdown parsing and the rules of [rumdl](https://github.com/rvben/rumdl) 0.2.78, cut down to
exactly what [reforge-cli](https://github.com/AYI-GROUP/reforge-cli) (`rf`) uses: `LintContext`,
the 33 rules `rf docs validate` and `rf docs format` run, and the helpers those need.

Everything else is gone:

- the CLI, the LSP, the WASM build;
- configuration loading (files, `discovery`, `.editorconfig`, markdownlint configs) and the
  inline `<!-- rumdl-… -->` / `<!-- prettier-ignore -->` comments;
- every rule `rf` does not run;
- every flavor but standard markdown (MkDocs, MDX, Pandoc, Quarto, Obsidian, Kramdown, MyST,
  Hugo, Azure DevOps, MDG, gh-aw);
- the tests, the benches and the docs of the original project.

It is not meant to be used on its own, nor to be merged back: its public API is whatever `rf`
imports, and it changes when `rf` does.

## License

MIT, as the original: see [LICENSE](LICENSE).
