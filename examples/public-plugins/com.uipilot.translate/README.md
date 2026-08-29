# UiPilot Youdao Translate Plugin

This independently removable public plugin translates English to Simplified Chinese and Simplified Chinese to English through the Host-managed HTTPS API.

## Development Credential Notice

The MVP Runtime intentionally contains development-only provider credentials. Anyone who can inspect the plugin package can read and reuse them. Do not distribute this build as a production plugin. UiPilot Public Plugin API v1 does not currently expose production secret injection.

## Install And Use

1. In UiPilot's public plugin panel, choose **Development directory**.
2. Select the `package` directory beside this README.
3. Confirm clipboard access and Host-managed HTTPS access to `openapi.youdao.com`.
4. Run `/translate Hello` or `/translate 你好` and press Enter.
5. Press Enter again on a successful result to copy the translation.

The plugin stores no translation history and performs no background requests.

## Verify

```powershell
node --test examples/public-plugins/com.uipilot.translate/tests/runtime.test.js
npm exec tsc -- --ignoreConfig --noEmit --strict examples/public-plugins/com.uipilot.translate/tests/sdk-contract.ts
node packages/plugin-cli/dist/cli.mjs validate examples/public-plugins/com.uipilot.translate/package --platform windows
```

The Runtime uses only `api.network.request(...)`. The provider endpoint and V3 signing algorithm follow the official [Youdao text translation API documentation](https://ai.youdao.com/DOCSIRMA/html/trans/api/wbfy/index.html).
