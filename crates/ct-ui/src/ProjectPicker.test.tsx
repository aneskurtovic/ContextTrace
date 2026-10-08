import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import ProjectPicker from './ProjectPicker';
import type { ProjectOption } from './types';

const options: ProjectOption[] = [
  { path: 'C:/repos/tmp-project', label: 'tmp-project', count: 5, temporary: false, directoryState: 'available' },
  { path: 'C:/Users/me/AppData/Local/Temp/.tmp1', label: '.tmp1', count: 2, temporary: true, directoryState: 'missing' },
  { path: 'C:/Users/me/AppData/Local/Temp/.tmp2', label: '.tmp2', count: 1, temporary: true, directoryState: 'available' },
  { path: 'C:/repos/moved', label: 'moved', count: 3, temporary: false, directoryState: 'missing' },
];
afterEach(() => { cleanup(); window.localStorage.clear(); });

describe('project presentation preferences', () => {
  it('folds temporary folders into a counted group and keeps missing repositories visible', () => {
    const onChange = vi.fn();
    render(<ProjectPicker options={options} value={{ kind: 'any' }} onChange={onChange} />);
    const picker = screen.getByLabelText('Filter sessions by project') as HTMLSelectElement;
    expect(Array.from(picker.options).map((option) => option.text)).toEqual([
      'All projects', 'tmp-project · 5', 'moved · folder missing · 3', 'Temporary workspaces · 3',
    ]);
    fireEvent.change(picker, { target: { value: 'temporary' } });
    expect(onChange).toHaveBeenLastCalledWith({ kind: 'temporary' });
    fireEvent.click(screen.getByRole('button', { name: 'Expand temporary folders (2)' }));
    expect(picker.querySelector('optgroup')?.label).toBe('Temporary workspaces');
    expect(picker.querySelector(`option[value="${options[1].path}"]`)?.textContent).toContain('folder missing');
  });

  it('persists hide and always-show choices without changing the session filter', () => {
    const onChange = vi.fn();
    const view = render(<ProjectPicker options={options} value={{ kind: 'any' }} onChange={onChange} />);
    fireEvent.click(screen.getByText('Manage project visibility'));
    fireEvent.change(screen.getByLabelText(`Visibility for ${options[0].path}`), { target: { value: 'hidden' } });
    fireEvent.change(screen.getByLabelText(`Visibility for ${options[1].path}`), { target: { value: 'always' } });
    expect(onChange).not.toHaveBeenCalled();
    view.unmount();
    render(<ProjectPicker options={options} value={{ kind: 'any' }} onChange={onChange} />);
    const picker = screen.getByLabelText('Filter sessions by project') as HTMLSelectElement;
    expect(Array.from(picker.options).some((option) => option.value === options[0].path)).toBe(false);
    expect(Array.from(picker.options).some((option) => option.value === options[1].path)).toBe(true);
    fireEvent.click(screen.getByText('Manage project visibility'));
    fireEvent.change(screen.getByLabelText(`Visibility for ${options[0].path}`), { target: { value: 'default' } });
    expect(Array.from(picker.options).some((option) => option.value === options[0].path)).toBe(true);
  });

  it('keeps a selected folded or hidden path explicit and can restore preferences outside current results', () => {
    window.localStorage.setItem('ct.projectVisibility', JSON.stringify({ [options[1].path!]: 'hidden' }));
    render(<ProjectPicker options={[]} value={{ kind: 'path', path: options[1].path! }} onChange={vi.fn()} />);
    const picker = screen.getByLabelText('Filter sessions by project') as HTMLSelectElement;
    expect(picker.value).toBe(options[1].path);
    fireEvent.click(screen.getByText('Manage project visibility'));
    fireEvent.change(screen.getByLabelText(`Visibility for ${options[1].path}`), { target: { value: 'default' } });
    expect(JSON.parse(window.localStorage.getItem('ct.projectVisibility')!)).toEqual({});
  });
});
