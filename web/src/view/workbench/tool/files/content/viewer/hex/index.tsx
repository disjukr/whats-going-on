interface HexViewerContentProps {
  text: string;
}

export function HexViewerContent({ text }: HexViewerContentProps) {
  return <pre className="file-content binary">{text}</pre>;
}
