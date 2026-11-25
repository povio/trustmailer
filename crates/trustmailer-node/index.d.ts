export * from './binding'

import type { AttachmentInput, MailOptions } from './binding'

/** Anything an attachment's `open()` can resolve to. */
export type AttachmentSource =
  | ReadableStream<Uint8Array>
  | AsyncIterable<Uint8Array>
  | Uint8Array

/** An attachment whose bytes are read only while its MIME part is being written. */
export interface LazyAttachmentInput {
  filename?: string
  contentType?: string
  /** Content-ID for `cid:` references; the attachment is then sent inline. */
  cid?: string
  content?: never
  /**
   * Opens the attachment's bytes. Resolve `null` to leave the attachment out of the
   * message entirely. A rejection, or a source that errors, errors the output stream.
   */
  open: () => Promise<AttachmentSource | null> | AttachmentSource | null
}

export interface StreamMailOptions extends Omit<MailOptions, 'attachments'> {
  attachments?: Array<AttachmentInput | LazyAttachmentInput>
}

/**
 * Renders a draft as RFC 5322 MIME bytes like `renderEml`, but streams attachment
 * bodies from `open()` sources instead of holding them in memory.
 *
 * Headers, boundaries, RFC 2047/2231 encoding and header sanitizing come from the
 * same native builder as `renderEml`; given the same input the output differs only in
 * `Date`, `Message-ID` (unless `messageId` is set) and the random boundaries. Every
 * attachment is base64 with 76-character CRLF lines.
 *
 * Lazy attachments are opened in the order their parts appear (inline `cid` parts
 * first, then regular attachments), each when the previous one starts streaming, so
 * at most one source is open ahead of the one being read. The first one is opened
 * before the promise resolves, so its failure can still become an HTTP error. The
 * stream pulls lazily (high-water mark 0); cancelling it cancels the source being read
 * and any opened ahead of it.
 */
export declare function renderEmlStream(
  options: StreamMailOptions,
): Promise<ReadableStream<Uint8Array>>
