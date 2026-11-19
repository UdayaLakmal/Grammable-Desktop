// Grammable — preset icons. Presets use SF Symbol names (shared with the macOS app);
// this maps them to emoji so the same preset files work everywhere.
const PRESET_ICONS = {
  "checkmark.seal": "✅", "checkmark": "✔️", "sparkles": "✨", "briefcase": "💼",
  "face.smiling": "😊", "bolt": "⚡", "scissors": "✂️", "text.badge.minus": "🔤",
  "list.bullet": "📋", "envelope": "✉️", "wand.and.stars": "🪄", "graduationcap": "🎓",
  "globe": "🌐", "character.book.closed": "📖", "bubble.left": "💬", "bubble.left.and.bubble.right": "💬",
  "message": "💬", "person.crop.circle": "👤", "hand.thumbsup": "👍", "lightbulb": "💡",
  "pencil": "✏️", "doc.text": "📄", "book": "📖", "flag": "🚩", "heart": "❤️",
  "star": "⭐", "lock": "🔒", "terminal": "⌨️", "chevron.left.forwardslash.chevron.right": "</>",
  "arrow.triangle.branch": "🔀", "network": "🔗", "megaphone": "📣", "text.quote": "❝",
  "translate": "🌐", "a.magnify": "🔍", "textformat": "🔠", "textformat.abc": "🔤",
};

function presetIcon(name) {
  if (!name) return "🪄";
  if (PRESET_ICONS[name]) return PRESET_ICONS[name];
  const base = name.replace(/\.(fill|circle|square)$/,"");
  if (PRESET_ICONS[base]) return PRESET_ICONS[base];
  // A preset may use an emoji directly.
  if (/\p{Extended_Pictographic}/u.test(name)) return name;
  return "🪄";
}
