# Community presets

Drop any of these files into `~/.grammable/presets/` and they appear in the Grammable menu instantly.

```bash
# install one preset
curl -fsSL https://raw.githubusercontent.com/UdayaLakmal/Grammable/main/presets/git-commit-message.md \
  -o ~/.grammable/presets/git-commit-message.md
```

| Preset | What it does | Mode |
|---|---|---|
| [`academic.md`](academic.md) | Formal academic tone | rewrite |
| [`british-english.md`](british-english.md) | Convert to British spelling & usage | fix |
| [`git-commit-message.md`](git-commit-message.md) | Turn notes into a conventional commit message | rewrite |
| [`linkedin-post.md`](linkedin-post.md) | Engaging LinkedIn post | rewrite |
| [`non-native-polish.md`](non-native-polish.md) | Natural English for non-native writers | fix |
| [`slack-reply.md`](slack-reply.md) | Short, friendly Slack message | rewrite |
| [`translate-to-english.md`](translate-to-english.md) | Translate any language to English | rewrite |
| [`grammar-offline.md`](grammar-offline.md) | Grammar fix pinned to local Ollama | fix |

## Contributing a preset

1. Copy an existing file and edit the front matter and instructions.
2. Keep instructions short and specific; say what to **keep** as well as what to change.
3. Test it on a few real texts with at least one backend.
4. Open a pull request adding the file and a row to the table above.
