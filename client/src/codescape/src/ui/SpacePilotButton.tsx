/**
 * SpacePilot connect button. WebHID needs a user gesture to pair the first
 * time; a paired device reconnects silently on load. Disabled with a reason
 * when the browser or context cannot do WebHID (http on a non-localhost host).
 */
import { useEffect, useState } from 'react';
import { hidAvailable, spacePilot, type SpacePilotState } from '../input/spacePilot';
import { useSpacePilotButtons } from '../input/SpacePilotRig';

export function SpacePilotButton() {
  const avail = hidAvailable();
  const dev = spacePilot();
  const [state, setState] = useState<SpacePilotState>(dev.state);
  const [name, setName] = useState(dev.productName);
  useSpacePilotButtons();

  useEffect(() => {
    dev.onState = (s, n) => {
      setState(s);
      setName(n);
    };
    if (dev.state === 'idle') void dev.resume();
    return () => {
      dev.onState = () => {};
    };
  }, [dev]);

  const title = !avail.ok
    ? avail.reason
    : state === 'connected'
      ? `${name || 'SpaceMouse'} connected · click to disconnect`
      : state === 'connecting'
        ? 'Choose the device in the browser prompt'
        : 'Connect a 3Dconnexion SpacePilot / SpaceMouse (WebHID)';

  return (
    <button
      className={`cs-spacepilot${state === 'connected' ? ' on' : ''}`}
      disabled={!avail.ok || state === 'connecting'}
      aria-pressed={state === 'connected'}
      aria-label="SpacePilot"
      title={title}
      onClick={() => {
        if (state === 'connected') void dev.disconnect();
        else void dev.request();
      }}
    >
      <svg width="20" height="20" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="13" r="6" fill="none" stroke="currentColor" strokeWidth="1.8" />
        <circle cx="12" cy="13" r="2.2" fill="currentColor" />
        <path d="M4 21h16" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
        <path d="M12 3v3" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
      </svg>
    </button>
  );
}
