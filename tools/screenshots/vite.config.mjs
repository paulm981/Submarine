// The app's own Vite config, with the screenshot mocks (mocks.js) loaded
// before the app's entry. Nothing here is part of the app's build.
import { mergeConfig } from "vite";
import appConfig from "../../vite.config.ts";

const mocksFirst = {
  name: "screenshot-mocks-first",
  transformIndexHtml(html) {
    return html.replace(
      '<script type="module" src="/src/main.tsx"></script>',
      '<script type="module">import "/tools/screenshots/mocks.js"; import "/src/main.tsx";</script>',
    );
  },
};

export default async () => {
  const app = await appConfig({ command: "serve", mode: "development" });
  return mergeConfig(app, {
    plugins: [mocksFirst],
    server: { port: 1430, strictPort: false, host: "127.0.0.1", hmr: false, fs: { allow: ["tools/screenshots"] } },
  });
};
