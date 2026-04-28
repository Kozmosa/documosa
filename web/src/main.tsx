import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource/material-symbols-outlined/400.css'
import './i18n'
import './index.css'
import App from './App.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
