/*
 * Resuming a zstd frame in the middle — SlotLab books index (src/books/zran.rs).
 *
 * The books of a Stake Engine game are ONE zstd frame of several GB. Reading
 * book N normally means decompressing everything before it. Between two
 * blocks of a frame, a decoder carries exactly four things (RFC 8878 §3.1.1):
 * the history window, the last Huffman table, the last FSE tables of literal
 * lengths / offsets / match lengths, and the three repeat offsets. Saved at a
 * block boundary and restored in a fresh decoder, they let decompression
 * restart there — so the books are indexed IN PLACE, without the full
 * re-compressed copy SlotLab used to keep next to them.
 *
 * libzstd has no public API for this, hence this file: it reads and writes
 * `ZSTD_DCtx` fields and is compiled against the sources of the libzstd it
 * links with (build.rs), so the layout always matches. Every field touched
 * here sits before the first conditional member of the struct. The Rust side
 * re-decodes every indexed book from these resume points before trusting
 * them, and falls back to a re-framed copy if one ever differs.
 */
#ifndef ZSTD_STATIC_LINKING_ONLY
#  define ZSTD_STATIC_LINKING_ONLY
#endif
#include <string.h>
#include "zstd.h"
#include "common/zstd_internal.h"
#include "decompress/zstd_decompress_internal.h"

typedef struct {
    U32 litEntropy;
    U32 fseEntropy;
    U32 rep[ZSTD_REP_NUM];
    U32 reserved;
    ZSTD_seqSymbol LL[SEQSYMBOL_TABLE_SIZE(LLFSELog)];
    ZSTD_seqSymbol OF[SEQSYMBOL_TABLE_SIZE(OffFSELog)];
    ZSTD_seqSymbol ML[SEQSYMBOL_TABLE_SIZE(MLFSELog)];
    HUF_DTable huf[HUF_DTABLE_SIZE(ZSTD_HUFFDTABLE_CAPACITY_LOG)];
} slz_state;

size_t slz_state_size(void) { return sizeof(slz_state); }
unsigned slz_header_version(void) { return ZSTD_VERSION_NUMBER; }
unsigned slz_lib_version(void) { return ZSTD_versionNumber(); }

ZSTD_DCtx* slz_create(void) { return ZSTD_createDCtx(); }
void slz_free(ZSTD_DCtx* d) { ZSTD_freeDCtx(d); }
size_t slz_begin(ZSTD_DCtx* d) { return ZSTD_decompressBegin(d); }
size_t slz_next_src_size(ZSTD_DCtx* d) { return ZSTD_nextSrcSizeToDecompress(d); }
size_t slz_continue(ZSTD_DCtx* d, void* dst, size_t cap, const void* src, size_t n)
{
    return ZSTD_decompressContinue(d, dst, cap, src, n);
}
unsigned slz_is_error(size_t code) { return ZSTD_isError(code); }
const char* slz_error_name(size_t code) { return ZSTD_getErrorName(code); }
int slz_stage(const ZSTD_DCtx* d) { return (int)d->stage; }
unsigned long long slz_window_size(const ZSTD_DCtx* d) { return d->fParams.windowSize; }
unsigned slz_checksum_flag(const ZSTD_DCtx* d) { return d->fParams.checksumFlag; }

/* A table in use may live elsewhere than in `entropy` (the predefined
 * distributions are static tables): copy what it points at, as many cells
 * as its header says. */
static void copy_table(ZSTD_seqSymbol* dst, const ZSTD_seqSymbol* src, U32 maxLog)
{
    U32 tableLog = ((const ZSTD_seqSymbol_header*)(const void*)src)->tableLog;
    if (tableLog > maxLog) tableLog = maxLog;
    memset(dst, 0, sizeof(ZSTD_seqSymbol) * SEQSYMBOL_TABLE_SIZE(maxLog));
    memcpy(dst, src, sizeof(ZSTD_seqSymbol) * (1 + ((size_t)1 << tableLog)));
}

/* Saves the entropy state between two blocks of a frame. -1 elsewhere. */
int slz_save(const ZSTD_DCtx* d, void* out)
{
    slz_state* s = (slz_state*)out;
    if (d->stage != ZSTDds_decodeBlockHeader) return -1;
    memset(s, 0, sizeof(*s));
    s->litEntropy = d->litEntropy;
    s->fseEntropy = d->fseEntropy;
    memcpy(s->rep, d->entropy.rep, sizeof(s->rep));
    copy_table(s->LL, d->LLTptr, LLFSELog);
    copy_table(s->OF, d->OFTptr, OffFSELog);
    copy_table(s->ML, d->MLTptr, MLFSELog);
    memcpy(s->huf, d->HUFptr, sizeof(s->huf));
    return 0;
}

/* Starts a decoder as if it had just decoded the `wlen` bytes of `window`
 * with the entropy state `state`. `window` must stay alive and unchanged
 * while the decoder may still reference it. The caller then feeds a frame
 * header (no content size, no checksum, the original window size) and the
 * original blocks from the resume point on. */
size_t slz_resume(ZSTD_DCtx* d, const void* state, const void* window, size_t wlen)
{
    const slz_state* s = (const slz_state*)state;
    size_t const r = ZSTD_decompressBegin(d);
    if (ZSTD_isError(r)) return r;
    if (wlen) {   /* what ZSTD_refDictContent() does for a raw-content dictionary */
        d->prefixStart = window;
        d->virtualStart = window;
        d->dictEnd = (const BYTE*)window + wlen;
        d->previousDstEnd = d->dictEnd;
    }
    memcpy(d->entropy.LLTable, s->LL, sizeof(s->LL));
    memcpy(d->entropy.OFTable, s->OF, sizeof(s->OF));
    memcpy(d->entropy.MLTable, s->ML, sizeof(s->ML));
    memcpy(d->entropy.hufTable, s->huf, sizeof(s->huf));
    memcpy(d->entropy.rep, s->rep, sizeof(s->rep));
    d->LLTptr = d->entropy.LLTable;
    d->OFTptr = d->entropy.OFTable;
    d->MLTptr = d->entropy.MLTable;
    d->HUFptr = d->entropy.hufTable;
    d->litEntropy = s->litEntropy;
    d->fseEntropy = s->fseEntropy;
    return 0;
}
