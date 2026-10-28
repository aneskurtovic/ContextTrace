import { useState } from 'react';
import { projectName } from './format';
import type { ProjectFilter, ProjectOption } from './types';

type Preference = 'hidden' | 'always';
const STORAGE_KEY = 'ct.projectVisibility';

function readPreferences(): Record<string, Preference> {
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? '{}');
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
    return Object.fromEntries(Object.entries(value).filter(([, preference]) => preference === 'hidden' || preference === 'always'));
  } catch { return {}; }
}

function label(option: ProjectOption): string {
  return `${option.label}${option.directoryState === 'missing' ? ' · folder missing' : ''} · ${option.count}`;
}

export default function ProjectPicker({ options, value, onChange }: {
  options: ProjectOption[];
  value: ProjectFilter;
  onChange: (value: ProjectFilter) => void;
}) {
  const [preferences, setPreferences] = useState(readPreferences);
  const [expanded, setExpanded] = useState(false);
  const [storageError, setStorageError] = useState<string | null>(null);
  const preference = (option: ProjectOption) => option.path == null ? undefined : preferences[option.path];
  const main = options.filter((option) => preference(option) !== 'hidden' && (!option.temporary || preference(option) === 'always'));
  const temporary = options.filter((option) => option.temporary);
  const folded = temporary.filter((option) => !preference(option));
  const shown = [...main, ...(expanded ? folded : [])];
  const selectedMissing = value.kind === 'path' && !shown.some((option) => option.path === value.path);
  const current = value.kind === 'path' ? options.find((option) => option.path === value.path) : undefined;

  function update(path: string, preference: string) {
    const next = { ...preferences };
    if (preference === 'hidden' || preference === 'always') next[path] = preference;
    else delete next[path];
    setPreferences(next);
    try { window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next)); setStorageError(null); }
    catch { setStorageError('The visibility setting applies now, but could not be saved for the next launch.'); }
  }

  return <>
    <select aria-label="Filter sessions by project" value={value.kind === 'path' ? value.path : value.kind}
      onChange={(event) => {
        const chosen = event.target.value;
        onChange(chosen === 'any' || chosen === 'unrecorded' || chosen === 'temporary' ? { kind: chosen } : { kind: 'path', path: chosen });
      }}>
      <option value="any">All projects</option>
      {selectedMissing && value.kind === 'path' && <option value={value.path} title={value.path}>
        {current ? `${label(current)} · selected` : `${projectName(value.path)} · no sessions for this agent`}
      </option>}
      {main.map((option) => <option key={option.path ?? 'unrecorded'} value={option.path ?? 'unrecorded'} title={option.path ?? undefined}>{label(option)}</option>)}
      {(temporary.length > 0 || value.kind === 'temporary') && <option value="temporary">
        Temporary workspaces · {temporary.reduce((sum, option) => sum + option.count, 0)}
      </option>}
      {expanded && folded.length > 0 && <optgroup label="Temporary workspaces">
        {folded.map((option) => <option key={option.path} value={option.path!} title={option.path!}>{label(option)}</option>)}
      </optgroup>}
    </select>
    <div className="project-controls">
      {temporary.length > 0 && <button type="button" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
        {expanded ? 'Collapse' : 'Expand'} temporary folders ({temporary.length})
      </button>}
      <details className="project-manager">
        <summary>Manage project visibility</summary>
        <p>These settings only change the project list. Sessions remain searchable and included in totals.</p>
        {options.filter((option) => option.path != null).map((option) => <label key={option.path}>
          <span title={option.path!}>{option.label}{option.temporary ? ' · temporary' : ''}{option.directoryState === 'missing' ? ' · folder missing' : ''}<small>{option.path}</small></span>
          <select aria-label={`Visibility for ${option.path}`} value={preference(option) ?? 'default'} onChange={(event) => update(option.path!, event.target.value)}>
            <option value="default">Default</option>
            <option value="hidden">Hide from project list</option>
            <option value="always">Always show</option>
          </select>
        </label>)}
        {Object.entries(preferences).filter(([path]) => !options.some((option) => option.path === path)).map(([path, pref]) => <label key={path}>
          <span>{projectName(path)}<small>{path} · outside current results</small></span>
          <select aria-label={`Visibility for ${path}`} value={pref} onChange={(event) => update(path, event.target.value)}>
            <option value="default">Default</option><option value="hidden">Hide from project list</option><option value="always">Always show</option>
          </select>
        </label>)}
        {storageError && <p role="alert">{storageError}</p>}
      </details>
    </div>
  </>;
}
