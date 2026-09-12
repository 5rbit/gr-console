import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
// 폰트는 로컬 번들 — CDN 없이 dist 에 포함된다.
import 'pretendard/dist/web/variable/pretendardvariable-dynamic-subset.css'
import '@fontsource-variable/jetbrains-mono'
import './app.css'
import { App } from './App'

createRoot(document.getElementById('app')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
