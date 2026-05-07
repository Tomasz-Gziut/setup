import { execSync, spawn } from 'child_process';
import { InstalledApp } from '../types/config';

export class WingetService {
  private runCommand(command: string): string {
    try {
      return execSync(command, {
        encoding: 'utf-8',
        maxBuffer: 50 * 1024 * 1024,
        windowsHide: true
      });
    } catch (error: any) {
      if (error.stdout) {
        return error.stdout;
      }
      throw error;
    }
  }

  getInstalledApps(): InstalledApp[] {
    const rawOutput = this.runCommand('winget list --disable-interactivity');
    // Clean up Windows line endings
    const cleanOutput = rawOutput.replace(/\r/g, '');
    const lines = cleanOutput.split('\n');

    // Find separator line (all dashes) to detect column widths
    let separatorIndex = -1;
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].match(/^-{20,}$/)) {
        separatorIndex = i;
        break;
      }
    }

    if (separatorIndex === -1) {
      return [];
    }

    // Find header line (should contain Name and Id)
    let headerLine = '';
    for (let i = separatorIndex - 1; i >= 0; i--) {
      if (lines[i].includes('Name') && lines[i].includes('Id')) {
        // Extract just the header portion (after any spinner animation)
        const nameIdx = lines[i].lastIndexOf('Name');
        headerLine = lines[i].substring(nameIdx);
        break;
      }
    }

    if (!headerLine) {
      return [];
    }

    // Find column positions from header
    const idPos = headerLine.indexOf('Id');
    const versionPos = headerLine.indexOf('Version');
    const availablePos = headerLine.indexOf('Available');
    const sourcePos = headerLine.indexOf('Source');

    if (idPos === -1 || versionPos === -1) {
      return [];
    }

    const apps: InstalledApp[] = [];

    // Parse each app line (after separator)
    for (let i = separatorIndex + 1; i < lines.length; i++) {
      const line = lines[i];
      if (!line.trim() || line.includes('upgrades available')) continue;

      // Extract based on column positions
      const name = line.substring(0, idPos).trim();
      const id = line.substring(idPos, versionPos).trim();

      let version: string;
      let source: string;

      if (availablePos !== -1 && sourcePos !== -1) {
        version = line.substring(versionPos, availablePos).trim();
        source = line.substring(sourcePos).trim();
      } else if (sourcePos !== -1) {
        version = line.substring(versionPos, sourcePos).trim();
        source = line.substring(sourcePos).trim();
      } else {
        version = line.substring(versionPos).trim();
        source = '';
      }

      if (name && id) {
        apps.push({ name, id, version, source });
      }
    }

    return apps;
  }

  async isAvailableInWinget(id: string): Promise<boolean> {
    try {
      const output = this.runCommand(`winget search --id "${id}" --exact --disable-interactivity`);
      return output.includes(id);
    } catch {
      return false;
    }
  }

  async installApp(id: string): Promise<{ success: boolean; message: string }> {
    return new Promise((resolve) => {
      const process = spawn('winget', ['install', '--id', id, '--accept-source-agreements', '--accept-package-agreements', '--disable-interactivity'], {
        shell: true,
        stdio: ['ignore', 'pipe', 'pipe']
      });

      let output = '';
      let errorOutput = '';

      process.stdout?.on('data', (data) => {
        output += data.toString();
        // Print progress
        const text = data.toString().trim();
        if (text) {
          console.log(`  ${text}`);
        }
      });

      process.stderr?.on('data', (data) => {
        errorOutput += data.toString();
      });

      process.on('close', (code) => {
        if (code === 0) {
          resolve({ success: true, message: 'Installed successfully' });
        } else if (output.includes('already installed')) {
          resolve({ success: true, message: 'Already installed' });
        } else {
          resolve({ success: false, message: errorOutput || output || 'Installation failed' });
        }
      });

      process.on('error', (error) => {
        resolve({ success: false, message: error.message });
      });
    });
  }

  async searchApp(query: string): Promise<InstalledApp[]> {
    try {
      const output = this.runCommand(`winget search "${query}" --disable-interactivity`);
      const lines = output.split('\n');

      let headerIndex = -1;
      for (let i = 0; i < lines.length; i++) {
        if (lines[i].includes('Name') && lines[i].includes('Id')) {
          headerIndex = i;
          break;
        }
      }

      if (headerIndex === -1) return [];

      const separatorLine = lines[headerIndex + 1];
      if (!separatorLine?.includes('-')) return [];

      const columns: { start: number; end: number }[] = [];
      let inDash = false;
      let start = 0;

      for (let i = 0; i <= separatorLine.length; i++) {
        const char = separatorLine[i];
        if (char === '-' && !inDash) {
          inDash = true;
          start = i;
        } else if (char !== '-' && inDash) {
          inDash = false;
          columns.push({ start, end: i });
        }
      }

      const apps: InstalledApp[] = [];

      for (let i = headerIndex + 2; i < lines.length; i++) {
        const line = lines[i];
        if (!line.trim()) continue;

        if (columns.length >= 2) {
          const name = line.substring(columns[0].start, columns[0].end).trim();
          const id = line.substring(columns[1].start, columns[1].end).trim();
          const version = columns.length >= 3
            ? line.substring(columns[2].start, columns[2].end).trim()
            : '';
          const source = columns.length >= 4
            ? line.substring(columns[3].start, columns[3].end || line.length).trim()
            : '';

          if (name && id && !name.includes('---')) {
            apps.push({ name, id, version, source });
          }
        }
      }

      return apps;
    } catch {
      return [];
    }
  }
}
