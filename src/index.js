const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

document.getElementsByTagName('a')[0].onclick = () => invoke('open_path', { path: 'https://github.com/Tobiichi-Origuchi/CefDetectorLinux', isDir: false });

let cnt = 0;
let totalSize = 0;
const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];


const prettySize = len => {
  let order = 0;
  while (len >= 1024 && order < sizes.length - 1) {
    order++;
    len /= 1024;
  }
  return len.toFixed(2) + ' ' + sizes[order];
};

const nodes = [];
const mainElm = document.getElementsByTagName('main')[0];
const titleElm = document.getElementsByTagName('h2')[0];

const addAppFromRust = async (appInfo) => {
  console.log('Found:', appInfo.app_type, appInfo.file);
  totalSize += appInfo.size;

  const elm = document.createElement('section');
  const pathParts = appInfo.file.split('/');
  const fileName = pathParts[pathParts.length - 1];
  elm.title = appInfo.file;
  nodes.push([appInfo.size, elm]);

  const icon = await invoke('get_app_icon', { path: appInfo.file });
  elm.innerHTML = (icon ? `<img src="data:image/png;base64,${icon}" alt="${fileName}">` : '<h3>?</h3>') +
    `<h6 class=${!appInfo.is_dir && appInfo.is_running ? 'running' : ''}>${fileName}</h6><p>${appInfo.app_type}</p><sub>${prettySize(appInfo.size)}</sub>`;

  elm.onclick = () => {
    invoke('open_path', { path: appInfo.file, isDir: appInfo.is_dir });
  };
  mainElm.appendChild(elm);

  titleElm.innerText = `这台电脑上已找到 ${++cnt} 个 Chromium 内核的应用 (${prettySize(totalSize)}) - 搜索中...`;
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
  titleElm.className = 'running';
});

// Start the search
titleElm.innerText = '正在全盘搜索 CEF 应用，请耐心等待...';
invoke('start_search');
