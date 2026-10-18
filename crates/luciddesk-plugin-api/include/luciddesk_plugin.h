#ifndef LUCIDDESK_PLUGIN_H
#define LUCIDDESK_PLUGIN_H
#include <stdint.h>
#include <stddef.h>
#define LUCIDDESK_ABI_VERSION 1u
#define LUCIDDESK_PROTOCOL_VERSION 1u
#define LUCIDDESK_STATUS_OK 0
#define LUCIDDESK_STATUS_ERROR (-1)
#define LUCIDDESK_MAX_MESSAGE_BYTES (8u * 1024u * 1024u)

/* UTF-8 JSON bytes, borrowed only until the synchronous call returns. */
typedef void (*LucidDeskReply)(void *user, const uint8_t *data, size_t len);
typedef int32_t (*LucidDeskInvoke)(void *context, const uint8_t *data,
    size_t len, LucidDeskReply reply, void *user);
typedef struct LucidDeskPluginV1 {
    uint32_t abi_version;
    uint32_t struct_size;
    LucidDeskInvoke invoke;
} LucidDeskPluginV1;

#ifdef __cplusplus
extern "C" {
#endif
/* Export this exact symbol using the C calling convention. Return NULL on mismatch. */
const LucidDeskPluginV1 *luciddesk_plugin_v1(uint32_t host_abi_version);
#ifdef __cplusplus
}
#endif
#endif
