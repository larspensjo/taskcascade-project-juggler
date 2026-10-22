# Desktop icon

The vector master is `frontend/public/favicon.svg`.

From the repository root, regenerate with Tauri CLI 2:

```powershell
npx --yes @tauri-apps/cli@2 icon frontend/public/favicon.svg -o "<temp dir>\icons"
```

Replace `<temp dir>` with a temporary directory. Copy only the generated
`icon.ico` to `desktop/icons/icon.ico`; leave the other generated files out
of the repository. The CLI accepted the SVG directly; no PNG fallback was needed.
