'use strict';

// Tag the document with the OS before first paint so platform styles apply at once.
document.documentElement.dataset.platform = /Mac/.test(navigator.platform)
  ? 'macos'
  : /Win/.test(navigator.platform) ? 'windows' : 'linux';

document.addEventListener('contextmenu', (e) => e.preventDefault());
