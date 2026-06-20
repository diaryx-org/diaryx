import { describe, expect, it } from 'vitest';
import { describePublishError } from './publishErrors';

describe('describePublishError', () => {
  it('maps a missing workspace file (os error 2) to a clear, named message', () => {
    const r = describePublishError(
      "Failed to read file 'Adam's Archive.md': No such file or directory (os error 2)",
    );
    expect(r.title).toBe(`Couldn't read "Adam's Archive.md"`);
    expect(r.detail).toContain('moved or deleted');
    // Raw error is preserved for debugging.
    expect(r.detail).toContain('os error 2');
    expect(r.raw).toContain('No such file or directory');
  });

  it('maps a missing root index to root-index guidance', () => {
    const r = describePublishError('Could not find the workspace root index');
    expect(r.title).toBe('Workspace root index not found');
    expect(r.detail).toContain('contents');
  });

  it('maps "No active workspace" to root-index guidance', () => {
    const r = describePublishError('No active workspace');
    expect(r.title).toBe('Workspace root index not found');
  });

  it('maps permission errors to a workspace-access message', () => {
    const r = describePublishError('Permission denied (os error 13)');
    expect(r.title).toBe('Lost access to your workspace folder');
  });

  it('maps auth failures to a sign-in message', () => {
    expect(describePublishError('Unauthorized').title).toBe(
      'Your session expired — sign in again',
    );
    expect(describePublishError('request failed: 401').title).toBe(
      'Your session expired — sign in again',
    );
  });

  it('maps network failures to a reachability message', () => {
    expect(describePublishError('TypeError: Failed to fetch').title).toBe(
      "Couldn't reach the publishing server",
    );
    expect(describePublishError('connection refused').title).toBe(
      "Couldn't reach the publishing server",
    );
  });

  it('maps server build failures', () => {
    const r = describePublishError(
      'server-side render (build) failed: something went wrong',
    );
    expect(r.title).toBe('The server failed to build your site');
    expect(r.detail).toContain('uploaded');
  });

  it('falls back to the raw message for unknown errors', () => {
    const r = describePublishError('weird unexpected thing');
    expect(r.title).toBe('weird unexpected thing');
    expect(r.raw).toBe('weird unexpected thing');
  });

  it('falls back to the provided fallback for empty errors', () => {
    const r = describePublishError(null, 'Publish failed');
    expect(r.title).toBe('Publish failed');
  });

  it('accepts Error instances', () => {
    const r = describePublishError(
      new Error("Failed to read file 'notes/x.md': No such file or directory (os error 2)"),
    );
    expect(r.title).toBe(`Couldn't read "notes/x.md"`);
  });
});
