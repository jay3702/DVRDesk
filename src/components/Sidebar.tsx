import { NavLink } from 'react-router-dom';
import { useStore } from '../store/useStore';
import './Sidebar.css';

const NAV_ITEMS = [
  { to: '/',         label: 'Recent',      icon: '🕐' },
  { to: '/live',     label: 'Live',        icon: '📡' },
  { to: '/tv',       label: 'TV Shows',    icon: '📺' },
  { to: '/movies',   label: 'Movies',      icon: '🎬' },
  { to: '/library',  label: 'Videos',      icon: '📁' },
  { to: '/search',   label: 'Search',      icon: '🔎' },
  { to: '/settings', label: 'Settings',    icon: '⚙' },
];

export default function Sidebar() {
  const servers = useStore((s) => s.servers);
  const activeServerId = useStore((s) => s.activeServerId);
  const setActiveServer = useStore((s) => s.setActiveServer);

  return (
    <nav className="sidebar" aria-label="Main navigation">
      <div className="sidebar__brand">DVRDesk</div>
      <div className="sidebar__server-picker">
        <label className="sidebar__server-label" htmlFor="sidebar-server-select">
          Server
        </label>
        <select
          id="sidebar-server-select"
          className="sidebar__server-select"
          value={activeServerId}
          onChange={(e) => setActiveServer(e.target.value)}
          aria-label="Select DVR server"
        >
          {servers.map((server) => (
            <option key={server.id} value={server.id}>
              {server.name}
            </option>
          ))}
        </select>
      </div>
      <ul className="sidebar__nav">
        {NAV_ITEMS.map(({ to, label, icon }) => (
          <li key={to}>
            <NavLink
              to={to}
              end={to === '/'}
              className={({ isActive }) =>
                `sidebar__link ${isActive ? 'sidebar__link--active' : ''}`
              }
            >
              <span className="sidebar__icon" aria-hidden="true">{icon}</span>
              <span>{label}</span>
            </NavLink>
          </li>
        ))}
      </ul>
    </nav>
  );
}
