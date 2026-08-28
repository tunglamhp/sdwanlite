import { useState } from "react";
import {
  LayoutDashboard,
  Router,
  GitBranch,
  Tag,
  FileJson,
  Shield,
  Gauge,
  Network,
  Stethoscope,
  Settings,
  Sun,
  Moon,
} from "lucide-react";
import {
  BrowserRouter,
  Link,
  useLocation,
  Route,
  Routes,
} from "react-router-dom";
import Dashboard from "./pages/Dashboard";
import Devices from "./pages/Devices";
import Topology from "./pages/Topology";
import PathLabels from "./pages/PathLabels";
import Policies from "./pages/Policies";
import Firewall from "./pages/Firewall";
import QoS from "./pages/QoS";
import BGP from "./pages/BGP";
import Diagnostics from "./pages/Diagnostics";
import SettingsPage from "./pages/Settings";

const nav = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/devices", label: "Devices", icon: Router },
  { to: "/topology", label: "Topology", icon: GitBranch },
  { to: "/path-labels", label: "Path Labels", icon: Tag },
  { to: "/policies", label: "Policies", icon: FileJson },
  { to: "/firewall", label: "Firewall", icon: Shield },
  { to: "/qos", label: "QoS", icon: Gauge },
  { to: "/bgp", label: "BGP", icon: Network },
  { to: "/diagnostics", label: "Diagnostics", icon: Stethoscope },
  { to: "/settings", label: "Settings", icon: Settings },
];

function Sidebar({ theme, toggle }: { theme: "light" | "dark"; toggle: () => void }) {
  const loc = useLocation();
  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="logo">SD-WAN</span>
        <span className="sub">Control Plane</span>
      </div>
      <nav className="menu">
        {nav.map((item) => {
          const Icon = item.icon;
          const active = loc.pathname === item.to || (item.to !== "/" && loc.pathname.startsWith(item.to));
          return (
            <Link key={item.to} to={item.to} className={`menu-item ${active ? "active" : ""}`}>
              <Icon size={16} />
              <span>{item.label}</span>
            </Link>
          );
        })}
      </nav>
      <button className="theme-toggle" onClick={toggle}>
        {theme === "light" ? <Moon size={16} /> : <Sun size={16} />}
      </button>
    </aside>
  );
}

function Layout() {
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const toggle = () => setTheme((t) => (t === "light" ? "dark" : "light"));
  return (
    <div className={`app ${theme}`}>
      <BrowserRouter>
        <Sidebar theme={theme} toggle={toggle} />
        <main className="content">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/devices" element={<Devices />} />
            <Route path="/topology" element={<Topology />} />
            <Route path="/path-labels" element={<PathLabels />} />
            <Route path="/policies" element={<Policies />} />
            <Route path="/firewall" element={<Firewall />} />
            <Route path="/qos" element={<QoS />} />
            <Route path="/bgp" element={<BGP />} />
            <Route path="/diagnostics" element={<Diagnostics />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="*" element={<Dashboard />} />
          </Routes>
        </main>
      </BrowserRouter>
    </div>
  );
}

export default Layout;
