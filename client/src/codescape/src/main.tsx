import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { Session } from './session';
import './styles.css';

const el = document.getElementById('root');
if (!el) throw new Error('#root missing');
// One session per page; created outside React so StrictMode's double effects only restart sockets.
const session = new Session();
createRoot(el).render(
  <StrictMode>
    <App session={session} />
  </StrictMode>,
);
