#include <dispatch/dispatch.h>
#include <limits.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <xpc/xpc.h>
#import <Foundation/NSObject.h>

@interface MexericaScreenReply : NSObject {
@public
    dispatch_queue_t queue;
    xpc_connection_t peer;
    dispatch_semaphore_t completed;
    char *body;
    bool stopped;
}
@end

@implementation MexericaScreenReply
- (void)dealloc {
    free(body);
}
@end

void *mexerica_screen_xpc_send(const char *service, const char *requirement, const char *json) {
    MexericaScreenReply *call = [MexericaScreenReply new];
    call->queue = dispatch_queue_create("app.mexerica.screen.xpc", DISPATCH_QUEUE_SERIAL);
    call->peer = xpc_connection_create_mach_service(service, call->queue, 0);
    if (!call->peer) return NULL;
    if (__builtin_available(macOS 12.0, *)) {
        if (xpc_connection_set_peer_code_signing_requirement(call->peer, requirement) != 0) {
            xpc_connection_cancel(call->peer);
            return NULL;
        }
    } else {
        xpc_connection_cancel(call->peer);
        return NULL;
    }
    call->completed = dispatch_semaphore_create(0);
    xpc_connection_set_event_handler(call->peer, ^(xpc_object_t event) { (void)event; });
    xpc_connection_activate(call->peer);
    xpc_object_t request = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_string(request, "json", json);
    __weak MexericaScreenReply *weakCall = call;
    xpc_connection_send_message_with_reply(call->peer, request, call->queue, ^(xpc_object_t reply) {
        MexericaScreenReply *current = weakCall;
        if (!current || current->stopped) return;
        if (xpc_get_type(reply) == XPC_TYPE_DICTIONARY) {
            const char *body = xpc_dictionary_get_string(reply, "json");
            if (body && strnlen(body, 4 * 1024 * 1024) < 4 * 1024 * 1024) current->body = strdup(body);
        }
        dispatch_semaphore_signal(current->completed);
    });
    return (__bridge_retained void *)call;
}

char *mexerica_screen_xpc_finish(void *handle, unsigned seconds) {
    MexericaScreenReply *call = (__bridge_transfer MexericaScreenReply *)handle;
    const long timed_out = dispatch_semaphore_wait(call->completed, dispatch_time(DISPATCH_TIME_NOW, (int64_t)seconds * NSEC_PER_SEC));
    __block char *response = NULL;
    dispatch_sync(call->queue, ^{
        call->stopped = true;
        xpc_connection_cancel(call->peer);
        if (!timed_out) {
            response = call->body;
            call->body = NULL;
        }
    });
    return response;
}
