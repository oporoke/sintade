import { Injectable, InjectionToken, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';

import { components } from '../api/schema';
import { ApiClient } from './api-client.service';

type Schemas = components['schemas'];
export type ShareLink = Schemas['ShareLinkResponse'];
export type Visibility = Schemas['VisibilityDto'];
export type CreateLinkBody = Schemas['CreateLinkBody'];
export type UpdateLinkBody = Schemas['UpdateLinkBody'];

/** Managing a recording's share links (the owner's side of `/s/:slug`). */
export interface SharePort {
  list(recordingId: string): Promise<ShareLink[]>;
  create(recordingId: string, body: CreateLinkBody): Promise<ShareLink>;
  update(recordingId: string, linkId: string, body: UpdateLinkBody): Promise<ShareLink>;
  revoke(recordingId: string, linkId: string): Promise<void>;
}

@Injectable({ providedIn: 'root' })
export class ShareApi implements SharePort {
  private readonly api = inject(ApiClient);

  list(recordingId: string): Promise<ShareLink[]> {
    return firstValueFrom(this.api.get<ShareLink[]>(`${base(recordingId)}`));
  }

  create(recordingId: string, body: CreateLinkBody): Promise<ShareLink> {
    return firstValueFrom(this.api.post<ShareLink>(base(recordingId), body));
  }

  update(recordingId: string, linkId: string, body: UpdateLinkBody): Promise<ShareLink> {
    return firstValueFrom(
      this.api.patch<ShareLink>(`${base(recordingId)}/${encodeURIComponent(linkId)}`, body),
    );
  }

  async revoke(recordingId: string, linkId: string): Promise<void> {
    await firstValueFrom(
      this.api.delete<void>(`${base(recordingId)}/${encodeURIComponent(linkId)}`),
    );
  }
}

function base(recordingId: string): string {
  return `/recordings/${encodeURIComponent(recordingId)}/links`;
}

/** Overridable so pages can be tested without HTTP. */
export const SHARE_API = new InjectionToken<SharePort>('SharePort', {
  providedIn: 'root',
  factory: () => inject(ShareApi),
});

/** The address a viewer opens for a link. */
export function shareUrl(slug: string, origin: string = location.origin): string {
  return `${origin}/s/${slug}`;
}
