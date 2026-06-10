import { FileQuestion, FileText, Folder, HardDrive, Link2 } from "lucide-react";
import { FsEntry, FsEntryKind } from "../../../../../../protocol/rpc.ts";

interface EntryIconProps {
  entry: FsEntry;
}

export function EntryIcon({ entry }: EntryIconProps) {
  if (entry.kind === FsEntryKind.Directory) {
    return entry.path.endsWith("\\")
      ? <HardDrive size={16} />
      : <Folder size={16} />;
  }
  if (entry.kind === FsEntryKind.Symlink) return <Link2 size={16} />;
  if (entry.kind === FsEntryKind.File) return <FileText size={16} />;
  return <FileQuestion size={16} />;
}
