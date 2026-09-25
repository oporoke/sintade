/** Mirrors bin/api/src/routes/auth.rs and routes/me.rs response/request bodies. */

export interface MeUser {
  id: string;
  email: string;
  display_name: string;
  email_verified: boolean;
}

export interface MeWorkspace {
  id: string;
  name: string;
  role: string;
  is_personal: boolean;
}

export interface MeResponse {
  user: MeUser;
  workspaces: MeWorkspace[];
  current_workspace_id: string;
}

export interface MessageResponse {
  message: string;
}
