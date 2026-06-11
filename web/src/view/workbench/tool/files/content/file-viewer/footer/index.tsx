import { useBunja } from "bunja/react";
import { useAtom } from "jotai";
import { formatSize } from "../../../../../../../state/explorer.ts";
import {
  fileViewerImpls,
  isFileViewerImpl,
  textFileViewerImplId,
} from "../impl/index.ts";
import { fileViewerBunja } from "../state.tsx";

export function FileViewerFooter() {
  const viewer = useBunja(fileViewerBunja);
  const [impl, setImpl] = useAtom(viewer.implAtom);
  const disabled = impl === undefined;

  return (
    <div className="explorer-footer file-viewer-footer">
      <label className="file-viewer-impl-control">
        <span>View</span>
        <select
          className="file-viewer-impl-select"
          value={impl ?? textFileViewerImplId}
          disabled={disabled}
          aria-label="File viewer"
          onChange={(event) => {
            const value = event.currentTarget.value;
            if (isFileViewerImpl(value)) setImpl(value);
          }}
        >
          {fileViewerImpls.map((viewerImpl) => (
            <option key={viewerImpl.id} value={viewerImpl.id}>
              {viewerImpl.label}
            </option>
          ))}
        </select>
      </label>
      <span className="file-viewer-footer-size">
        {formatSize(viewer.fsEntry.size)}
      </span>
    </div>
  );
}
