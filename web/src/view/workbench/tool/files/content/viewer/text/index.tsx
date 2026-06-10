interface TextViewerContentProps {
  text: string;
}

export function TextViewerContent({ text }: TextViewerContentProps) {
  return <pre className="file-content">{text}</pre>;
}
