const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

document.getElementById('repo-link').onclick = (e) => {
  e.preventDefault();
  invoke('open_path', { path: 'https://github.com/Tobiichi-Origuchi/CefDetectorLinux', isDir: false });
};

let cnt = 0;
let totalSize = 0;
const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];

const prettySize = len => {
  if (len <= 0) return "0.00 B";
  let order = 0;
  let val = len;
  while (val >= 1024 && order < sizes.length - 1) {
    order++;
    val /= 1024;
  }
  return val.toFixed(2) + ' ' + sizes[order];
};

const nodes = [];
const mainElm = document.getElementById('main');
const titleElm = document.getElementById('title');

const addAppFromRust = (appInfo) => {
  console.log('Found:', appInfo.app_type, appInfo.file);
  totalSize += appInfo.size;
  cnt++;

  const elm = document.createElement('section');
  const pathParts = appInfo.file.split('/');
  const fileName = pathParts[pathParts.length - 1];
  elm.title = appInfo.file;
  nodes.push([appInfo.size, elm]);

  const isRunningCls = !appInfo.is_dir && appInfo.is_running ? 'running' : '';
  
  // Render placeholder immediately
  elm.innerHTML = `
    <div class="icon-container"><h3>?</h3></div>
    <h6 class="${isRunningCls}"></h6>
    <p></p>
    <sub>${prettySize(appInfo.size)}</sub>
  `;
  
  // XSS protection: safely set text content
  elm.querySelector('h6').textContent = fileName;
  elm.querySelector('p').textContent = appInfo.app_type;

  elm.onclick = () => {
    invoke('open_path', { path: appInfo.file, isDir: appInfo.is_dir });
  };
  mainElm.appendChild(elm);

  titleElm.innerText = `这台电脑上已找到 ${cnt} 个 Chromium 内核的应用 (${prettySize(totalSize)}) - 搜索中...`;

  // Fetch icon asynchronously and update DOM when ready
  invoke('get_app_icon', { path: appInfo.file }).then(icon => {
    if (icon) {
      const img = document.createElement('img');
      img.src = icon;
      img.alt = fileName;
      const iconContainer = elm.querySelector('.icon-container');
      iconContainer.innerHTML = '';
      iconContainer.appendChild(img);
    }
  }).catch(console.error);
};

// Listen for events from Rust
listen('app-found', (event) => {
  addAppFromRust(event.payload);
});

listen('search-done', () => {
  if (nodes.length) {
    nodes.sort(([a], [b]) => b - a).forEach(([_, elm], i) => (elm.style.order = i.toString()));
    titleElm.innerText = `搜索完成！这台电脑上总共有 ${cnt} 个 Chromium 内核的应用 (${prettySize(totalSize)})`;
  } else {
    titleElm.innerText = '搜索完成！这台电脑上没有 Chromium 内核的应用';
  }
  titleElm.className = 'completed';
});

// Start the search
invoke('start_search');

