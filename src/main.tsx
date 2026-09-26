import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app/App';
import { applyPlatformAttributes } from './app/platform';
import { initTheme } from './lib/theme';
// Inter backs up the native system fonts (SF Pro, Segoe UI) on other platforms.
import '@fontsource-variable/inter/opsz.css';
import './styles/index.css';

applyPlatformAttributes();
initTheme();

const root = document.getElementById('root');
if (!root) throw new Error('Root element #root not found');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
