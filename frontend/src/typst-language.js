// Minimal Typst highlighting via StreamLanguage: headings, #code calls,
// *strong*, _emph_, strings, comments. (The real Typst grammar ships as
// WASM, which we avoid for the offline single-file bundle.)
import { StreamLanguage } from "@codemirror/language";

const IDENT = /[\p{L}][\p{L}\p{N}_-]*/u;

export const typstLanguage = StreamLanguage.define({
  name: "typst",
  startState() {
    return { inBlockComment: false };
  },
  token(stream, state) {
    if (state.inBlockComment) {
      if (stream.match(/^.*?\*\//)) {
        state.inBlockComment = false;
      } else {
        stream.skipToEnd();
      }
      return "comment";
    }
    if (stream.match("/*")) {
      state.inBlockComment = true;
      return "comment";
    }
    if (stream.match("//")) {
      stream.skipToEnd();
      return "comment";
    }
    if (stream.sol() && stream.match(/^=+\s/)) {
      stream.skipToEnd();
      return "heading";
    }
    if (stream.match(/^"(?:[^"\\]|\\.)*"?/)) {
      return "string";
    }
    if (stream.match(/^#(let|set|show|import|include|if|else|for|while|context)\b/)) {
      return "keyword";
    }
    if (stream.match(new RegExp(`^#${IDENT.source}(\\.${IDENT.source})*`, "u"))) {
      return "variableName.function";
    }
    if (stream.match(/^\*[^*]+\*/)) {
      return "strong";
    }
    if (stream.match(/^_[^_]+_/)) {
      return "emphasis";
    }
    if (stream.match(/^\d+(\.\d+)?(pt|mm|cm|in|em|fr|%)?/)) {
      return "number";
    }
    stream.next();
    return null;
  },
  languageData: {
    commentTokens: { line: "//", block: { open: "/*", close: "*/" } },
  },
});
