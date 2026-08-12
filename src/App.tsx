import AppRouter from "./app/router";
import { useAuth } from "./auth/AuthContext";
import CreateProfile from "./auth/CreateProfile";
import LockScreen from "./auth/LockScreen";
import "./App.css";

function App() {
  const { status } = useAuth();

  switch (status) {
    case "loading":
      return <div className="splash">Loading…</div>;
    case "no-profile":
      return <CreateProfile />;
    case "locked":
      return <LockScreen />;
    case "unlocked":
      return <AppRouter />;
  }
}

export default App;
