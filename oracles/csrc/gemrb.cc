// Decodes one bare ACM the way GemRB's ACM importer does: its unpacker and subband decoder, driven by the importer's
// header parse and sample loop (shift by `levels`, truncate to 16 bits, stop at the header count).
#include <cstdlib>

#include "Streams/DataStream.h"
#include "harness.h"
#include "decoder.h"
#include "unpacker.h"

namespace {

unsigned read_le(const unsigned char* p, int bytes)
{
    unsigned v = 0;
    for (int i = bytes - 1; i >= 0; i--) {
        v = (v << 8) | p[i];
    }
    return v;
}

} // namespace

long harness_decode(const unsigned char* data, size_t len, short* out, size_t cap)
{
    if (len < 14 || read_le(data, 4) != 0x01032897) {
        return -1;
    }
    size_t samples_left = read_le(data + 4, 4);
    unsigned packed = read_le(data + 12, 2);
    int levels = packed & 15;
    int subblocks = packed >> 4;
    int block_size = (1 << levels) * subblocks;

    GemRB::DataStream stream(data + 14, len - 14);
    CValueUnpacker unpacker(levels, subblocks, &stream);
    CSubbandDecoder decoder(levels);
    if (!unpacker.init_unpacker() || !decoder.init_decoder()) {
        return -1;
    }
    int* block = static_cast<int*>(malloc(sizeof(int) * block_size));
    size_t got = 0;
    while (got < cap && samples_left > 0) {
        if (!unpacker.get_one_block(block)) {
            break;
        }
        decoder.decode_data(block, subblocks);
        size_t ready = static_cast<size_t>(block_size) > samples_left ? samples_left : block_size;
        samples_left -= ready;
        for (size_t i = 0; i < ready && got < cap; i++) {
            out[got++] = static_cast<short>(block[i] >> levels);
        }
    }
    free(block);
    return static_cast<long>(got);
}

int main(int argc, char** argv)
{
    return harness_main(argc, argv);
}
