const { rcedit } = require('rcedit');
const path = require('path');

const exePath = path.join(__dirname, 'setup.exe');
const manifestPath = path.join(__dirname, 'setup.exe.manifest');

async function addManifest() {
  try {
    await rcedit(exePath, {
      'application-manifest': manifestPath
    });
    console.log('Manifest added successfully - setup.exe now requires admin privileges');
  } catch (err) {
    console.error('Failed to add manifest:', err);
    process.exit(1);
  }
}

addManifest();
