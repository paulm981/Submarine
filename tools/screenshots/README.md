# Screenshots

The pictures of the fork's features in `docs/screenshots/fork/` come from the real page, run in a browser. The backend's calls are answered by Tauri's own mock (`@tauri-apps/api/mocks`) with made-up servers and addresses from the documentation ranges, so nothing here is a real server or a real person's data.

```sh
node tools/screenshots/take.mjs                      # every picture
node tools/screenshots/take.mjs bulk-push ai-levels  # some of them
```

It needs the app's `npm ci` and Playwright with Chromium (`require("playwright")` must work). Nothing here is part of the app's build.

- `vite.config.mjs`: the app's own Vite config, with `mocks.js` loaded before `src/main.tsx`.
- `mocks.js`: the canned answers. A call with none returns null, and `take.mjs` lists those calls after each picture.
- `take.mjs`: opens the page at 1400×900 (2× pixels), clicks to each screen, and saves the picture.
