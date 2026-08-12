import AppRouter from "./app/router";
import { useAuth } from "./auth/AuthContext";
import CreateProfile from "./auth/CreateProfile";
import LockScreen from "./auth/LockScreen";
import { Spinner } from "./ui";
// Owns .splash as well as the two auth screens' shell.
import "./auth/Auth.css";

function App() {
  const { status } = useAuth();

  switch (status) {
    case "loading":
      return (
        <div className="splash">
          <Spinner size="md" />
          Loading…
        </div>
      );
    case "no-profile":
      return <CreateProfile />;
    case "locked":
      return <LockScreen />;
    case "unlocked":
      return <AppRouter />;
  }
}

export default App;
