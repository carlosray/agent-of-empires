import { useEffect, useState } from "react";
import { fetchSettings, updateSettings } from "../lib/api";

interface SessionSettings {
  archive_on_delete?: boolean;
  archive_max_entries?: number;
}

function readSessionSettings(settings: Record<string, unknown> | null): SessionSettings {
  const session = settings?.session;
  if (!session || typeof session !== "object") return {};
  return session as SessionSettings;
}

export function ArchiveSettings() {
  const [archiveOnDelete, setArchiveOnDelete] = useState(true);
  const [archiveMaxEntries, setArchiveMaxEntries] = useState(100);
  const [sessionSettings, setSessionSettings] = useState<SessionSettings>({});

  useEffect(() => {
    fetchSettings().then((settings) => {
      const session = readSessionSettings(settings);
      setSessionSettings(session);
      setArchiveOnDelete(session.archive_on_delete ?? true);
      setArchiveMaxEntries(session.archive_max_entries ?? 100);
    });
  }, []);

  const save = async (patch: SessionSettings) => {
    const next = {
      ...sessionSettings,
      archive_on_delete: patch.archive_on_delete ?? archiveOnDelete,
      archive_max_entries: patch.archive_max_entries ?? archiveMaxEntries,
    };
    setSessionSettings(next);
    setArchiveOnDelete(next.archive_on_delete);
    setArchiveMaxEntries(next.archive_max_entries);
    await updateSettings({ session: next });
  };

  return (
    <div>
      <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-4">
        Archive
      </h3>

      <div className="space-y-4">
        <label className="flex items-center justify-between gap-3 cursor-pointer">
          <div>
            <div className="text-[13px] text-text-secondary">
              Archive on delete
            </div>
            <p className="text-[11px] text-text-muted mt-1">
              Deleted sessions are kept in the archive unless permanently deleted.
            </p>
          </div>
          <input
            type="checkbox"
            checked={archiveOnDelete}
            onChange={(e) => void save({ archive_on_delete: e.target.checked })}
            className="accent-brand-600 w-4 h-4 shrink-0"
          />
        </label>

        <div>
          <label className="block text-[13px] text-text-secondary mb-2">
            Max archived sessions
          </label>
          <input
            type="number"
            min={1}
            max={10000}
            value={archiveMaxEntries}
            onChange={(e) =>
              void save({
                archive_max_entries: Math.max(1, Number(e.target.value) || 1),
              })
            }
            className="bg-surface-800 border border-surface-700 rounded-md px-2 py-1 text-sm text-text-primary font-mono w-24"
          />
        </div>
      </div>
    </div>
  );
}
