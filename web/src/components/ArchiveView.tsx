import { useCallback, useEffect, useState } from "react";
import type { ArchivedSessionResponse } from "../lib/types";
import {
  deleteArchivedSession,
  fetchArchive,
  restoreArchivedSession,
} from "../lib/api";

interface Props {
  onClose: () => void;
  onRestored: () => void;
}

function formatDate(value: string): string {
  try {
    return new Intl.DateTimeFormat(undefined, {
      month: "short",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(value));
  } catch {
    return value;
  }
}

export function ArchiveView({ onClose, onRestored }: Props) {
  const [entries, setEntries] = useState<ArchivedSessionResponse[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    const data = await fetchArchive();
    setEntries(data ?? []);
    setLoading(false);
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void load();
    }, 0);
    return () => window.clearTimeout(timer);
  }, [load]);

  const restore = async (entry: ArchivedSessionResponse) => {
    const ok = await restoreArchivedSession(entry.id);
    if (ok) {
      onRestored();
      void load();
    }
  };

  const remove = async (entry: ArchivedSessionResponse) => {
    if (!window.confirm(`Permanently delete archived session "${entry.title}"?`)) {
      return;
    }
    const ok = await deleteArchivedSession(entry.id);
    if (ok) void load();
  };

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-surface-900">
      <div className="h-12 bg-surface-850 border-b border-surface-700 flex items-center px-4 shrink-0">
        <button
          onClick={onClose}
          className="text-brand-500 mr-3 cursor-pointer text-sm"
        >
          &larr; Back
        </button>
        <span className="text-sm font-semibold text-text-bright">Archive</span>
      </div>

      <div className="flex-1 overflow-auto">
        {loading ? (
          <div className="p-6 text-sm text-text-muted">Loading archive...</div>
        ) : entries.length === 0 ? (
          <div className="p-6 text-sm text-text-muted">No archived sessions</div>
        ) : (
          <table className="w-full text-sm">
            <thead className="sticky top-0 bg-surface-900 border-b border-surface-700/40 text-text-muted">
              <tr>
                <th className="text-left font-medium px-4 py-2">Session</th>
                <th className="text-left font-medium px-4 py-2 hidden md:table-cell">
                  Path
                </th>
                <th className="text-left font-medium px-4 py-2 hidden sm:table-cell">
                  Archived
                </th>
                <th className="text-right font-medium px-4 py-2">Actions</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((entry) => (
                <tr
                  key={entry.id}
                  className="border-b border-surface-700/20 hover:bg-surface-800/40"
                >
                  <td className="px-4 py-3 min-w-0">
                    <div className="text-text-primary truncate">{entry.title}</div>
                    <div className="font-mono text-[11px] text-text-dim truncate">
                      {entry.profile} · {entry.tool} · {entry.last_status}
                    </div>
                  </td>
                  <td className="px-4 py-3 hidden md:table-cell max-w-[420px]">
                    <div className="font-mono text-[12px] text-text-muted truncate">
                      {entry.project_path}
                    </div>
                  </td>
                  <td className="px-4 py-3 hidden sm:table-cell text-text-muted font-mono text-[12px]">
                    {formatDate(entry.archived_at)}
                  </td>
                  <td className="px-4 py-3">
                    <div className="flex justify-end gap-2">
                      <button
                        onClick={() => restore(entry)}
                        disabled={!entry.can_restore}
                        title={entry.restore_blocker ?? "Restore"}
                        className="h-8 px-3 rounded-md text-xs font-medium text-text-secondary hover:bg-surface-700/50 disabled:text-text-dim disabled:cursor-not-allowed cursor-pointer"
                      >
                        Restore
                      </button>
                      <button
                        onClick={() => remove(entry)}
                        className="h-8 px-3 rounded-md text-xs font-medium text-status-error hover:bg-surface-700/50 cursor-pointer"
                      >
                        Delete
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
