import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './i18n'
import './index.css'
import App from './App.tsx'
import SimpleEditorPage from './SimpleEditorPage.tsx'

const Root = window.location.pathname === '/simple/editor' ? SimpleEditorPage : App

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
