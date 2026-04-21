/**
 * Types + interface shared between the `CoreNamespaceService`
 * implementations (wasm-backed and Tauri-backed).
 *
 * These mirror the `serde` types in `diaryx_core::namespace` so wasm/IPC
 * return values deserialize directly into them. `namespaceService.ts`
 * re-exports the types from here for back-compat with existing callers.
 */

export interface NamespaceInfo {
  id: string;
  owner_user_id: string;
  created_at: number;
  metadata?: Record<string, unknown> | null;
}

export interface AudienceInfo {
  name: string;
  access: string;
}

export interface SubdomainInfo {
  subdomain: string;
  namespace_id: string;
}

export interface DomainInfo {
  domain: string;
  namespace_id: string;
  audience_name: string;
  created_at: number;
  verified: boolean;
}

export interface TokenResult {
  token: string;
}

export interface SubscriberInfo {
  id: string;
  email: string;
}

export interface BulkImportResult {
  added: number;
  errors: string[];
}

/**
 * Narrow surface the two concrete NamespaceServices (wasm, Tauri) implement.
 *
 * Mirrors the 15 free functions in `diaryx_core::namespace`. Each method
 * corresponds 1:1 to a Rust function; the JSON shapes for parameters and
 * return types match the sync server wire protocol.
 *
 * Error handling: every method throws an `AuthError`-shaped object on
 * non-2xx HTTP responses (re-using the auth surface since namespace ops
 * reuse the same `AuthenticatedClient` abstraction in Rust).
 */
export interface CoreNamespaceService {
  // CRUD
  getNamespace(id: string): Promise<NamespaceInfo>;
  createNamespace(
    id?: string | null,
    metadata?: Record<string, unknown> | null,
  ): Promise<NamespaceInfo>;
  updateNamespaceMetadata(
    id: string,
    metadata: Record<string, unknown> | null,
  ): Promise<NamespaceInfo>;
  deleteNamespace(id: string): Promise<void>;

  // Audiences
  listAudiences(id: string): Promise<AudienceInfo[]>;
  setAudience(id: string, name: string, access: string): Promise<void>;
  getAudienceToken(id: string, name: string): Promise<TokenResult>;

  // Subdomain
  claimSubdomain(
    id: string,
    subdomain: string,
    defaultAudience?: string | null,
  ): Promise<SubdomainInfo>;
  releaseSubdomain(id: string): Promise<void>;

  // Custom domains
  listDomains(id: string): Promise<DomainInfo[]>;
  registerDomain(
    id: string,
    domain: string,
    audienceName: string,
  ): Promise<DomainInfo>;
  removeDomain(id: string, domain: string): Promise<void>;

  // Subscribers
  listSubscribers(id: string, audience: string): Promise<SubscriberInfo[]>;
  addSubscriber(
    id: string,
    audience: string,
    email: string,
  ): Promise<SubscriberInfo>;
  removeSubscriber(
    id: string,
    audience: string,
    contactId: string,
  ): Promise<void>;
  bulkImportSubscribers(
    id: string,
    audience: string,
    emails: string[],
  ): Promise<BulkImportResult>;
}
