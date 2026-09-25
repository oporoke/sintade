import { components } from '../api/schema';

/**
 * Aliases over the DTOs generated from the API's OpenAPI contract (`just openapi`:
 * docs/api/openapi.json -> src/app/api/schema.ts). Never hand-edit shapes here; change the Rust
 * DTO and regenerate, and the compiler flags every affected call site.
 */
type Schemas = components['schemas'];

export type MeUser = Schemas['MeUser'];
export type MeWorkspace = Schemas['MeWorkspace'];
export type MeResponse = Schemas['MeResponse'];
export type MemberRole = Schemas['MemberRole'];
export type MessageResponse = Schemas['MessageResponse'];
export type Problem = Schemas['Problem'];
export type RegisterBody = Schemas['RegisterBody'];
export type LoginBody = Schemas['LoginBody'];
export type VerifyEmailBody = Schemas['VerifyEmailBody'];
export type ForgotPasswordBody = Schemas['ForgotPasswordBody'];
export type ResetPasswordBody = Schemas['ResetPasswordBody'];
export type UpdateProfileBody = Schemas['UpdateProfileBody'];
