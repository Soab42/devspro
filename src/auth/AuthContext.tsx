import { createContext, ReactNode, useContext, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface Profile {
  id: string;
  display_name: string;
  created_at: string;
}

type AuthStatus = "loading" | "no-profile" | "locked" | "unlocked";

interface AuthState {
  status: AuthStatus;
  profile: Profile | null;
  profiles: Profile[];
  error: string | null;
  createProfile: (displayName: string, password: string) => Promise<void>;
  unlock: (profileId: string, password: string) => Promise<void>;
  lock: () => Promise<void>;
}

const AuthContext = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AuthStatus>("loading");
  const [profile, setProfile] = useState<Profile | null>(null);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const hasProfile = await invoke<boolean>("auth_has_profile");
        if (!hasProfile) {
          setStatus("no-profile");
          return;
        }
        // Local lock is a convenience/profile-switch gate, not this app's real
        // security boundary — stay unlocked across restarts until the user
        // explicitly logs out, rather than re-prompting every relaunch.
        const session = await invoke<Profile | null>("auth_current_session");
        if (session) {
          setProfile(session);
          setStatus("unlocked");
          return;
        }
        const list = await invoke<Profile[]>("auth_list_profiles");
        setProfiles(list);
        setStatus("locked");
      } catch (e) {
        setError(String(e));
      }
    })();
  }, []);

  async function createProfile(displayName: string, password: string) {
    setError(null);
    try {
      const created = await invoke<Profile>("auth_create_profile", {
        displayName,
        password,
      });
      setProfiles([created]);
      setProfile(created);
      setStatus("unlocked");
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function unlock(profileId: string, password: string) {
    setError(null);
    try {
      const unlocked = await invoke<Profile>("auth_unlock", { profileId, password });
      setProfile(unlocked);
      setStatus("unlocked");
    } catch (e) {
      setError(String(e));
      throw e;
    }
  }

  async function lock() {
    try {
      await invoke("auth_logout");
    } finally {
      setProfile(null);
      setStatus("locked");
    }
  }

  return (
    <AuthContext.Provider value={{ status, profile, profiles, error, createProfile, unlock, lock }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth(): AuthState {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
