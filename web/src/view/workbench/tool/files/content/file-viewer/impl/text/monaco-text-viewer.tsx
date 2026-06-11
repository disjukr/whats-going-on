import { useEffect, useRef } from "react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api.js";

interface MonacoTextViewerProps {
  path: string;
  text: string;
}

const monacoHost = globalThis as typeof globalThis & {
  MonacoEnvironment?: {
    getWorker: (_moduleId: string, label: string) => Worker;
  };
};

monacoHost.MonacoEnvironment ??= {
  getWorker: (_moduleId, label) => {
    switch (label) {
      case "css":
      case "less":
      case "scss":
        return new Worker(new URL("./workers/css.worker.ts", import.meta.url), {
          type: "module",
        });
      case "handlebars":
      case "html":
      case "razor":
        return new Worker(
          new URL("./workers/html.worker.ts", import.meta.url),
          { type: "module" },
        );
      case "javascript":
      case "typescript":
        return new Worker(new URL("./workers/ts.worker.ts", import.meta.url), {
          type: "module",
        });
      case "json":
        return new Worker(
          new URL("./workers/json.worker.ts", import.meta.url),
          { type: "module" },
        );
      default:
        return new Worker(
          new URL("./workers/editor.worker.ts", import.meta.url),
          { type: "module" },
        );
    }
  },
};

export function MonacoTextViewer({ path, text }: MonacoTextViewerProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | undefined>(
    undefined,
  );
  const modelRef = useRef<monaco.editor.ITextModel | undefined>(undefined);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const model = monaco.editor.createModel(text, languageFromPath(path));
    const editor = monaco.editor.create(host, {
      automaticLayout: true,
      contextmenu: true,
      domReadOnly: true,
      fontFamily:
        'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace',
      fontSize: 12,
      lineHeight: 19,
      minimap: { enabled: false },
      model,
      readOnly: true,
      renderLineHighlight: "none",
      scrollBeyondLastLine: false,
      smoothScrolling: true,
      stickyScroll: { enabled: false },
      wordWrap: "off",
    });
    editorRef.current = editor;
    modelRef.current = model;

    return () => {
      editor.dispose();
      model.dispose();
      editorRef.current = undefined;
      modelRef.current = undefined;
    };
  }, []);

  useEffect(() => {
    const model = modelRef.current;
    if (!model || model.getValue() === text) return;
    model.setValue(text);
  }, [text]);

  useEffect(() => {
    const model = modelRef.current;
    if (!model) return;
    monaco.editor.setModelLanguage(model, languageFromPath(path));
  }, [path]);

  return <div ref={hostRef} className="monaco-text-viewer" />;
}

function languageFromPath(path: string): string {
  const basename = fileBasename(path).toLowerCase();
  if (basename === "dockerfile") return "dockerfile";
  if (basename === "makefile") return "makefile";

  const extension = fileExtension(basename);
  switch (extension) {
    case "c":
    case "h":
      return "c";
    case "cmd":
    case "bat":
      return "bat";
    case "cpp":
    case "hpp":
      return "cpp";
    case "cs":
      return "csharp";
    case "css":
      return "css";
    case "go":
      return "go";
    case "html":
      return "html";
    case "java":
      return "java";
    case "js":
    case "mjs":
      return "javascript";
    case "json":
      return "json";
    case "jsx":
      return "javascript";
    case "kt":
      return "kotlin";
    case "md":
      return "markdown";
    case "ps1":
      return "powershell";
    case "py":
      return "python";
    case "rs":
      return "rust";
    case "sh":
      return "shell";
    case "sql":
      return "sql";
    case "svg":
    case "xml":
      return "xml";
    case "toml":
      return "toml";
    case "ts":
      return "typescript";
    case "tsx":
      return "typescript";
    case "yaml":
    case "yml":
      return "yaml";
    default:
      return "plaintext";
  }
}

function fileBasename(path: string): string {
  const slashIndex = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return path.slice(slashIndex + 1);
}

function fileExtension(basename: string): string | undefined {
  const dotIndex = basename.lastIndexOf(".");
  if (dotIndex <= 0 || dotIndex === basename.length - 1) return undefined;
  return basename.slice(dotIndex + 1);
}
