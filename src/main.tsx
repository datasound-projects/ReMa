import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app/App';
import { applyPlatformAttributes } from './app/platform';
// Inter backs up the native system fonts (SF Pro, Segoe UI) on other platforms.
import '@fontsource-variable/inter/opsz.css';
import './styles/index.css';

applyPlatformAttributes();

const root = document.getElementById('root');
if (!root) throw new Error('Root element #root not found');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
