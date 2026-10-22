// Decodes one bare ACM with the Fallout 2 engine's decoder (fallout2-ce).
#include <cstring>

#include "harness.h"
#include "sound_decoder.h"

namespace {

struct Input {
    const unsigned char* data;
    size_t len;
    size_t pos;
};

int read_input(void* data, void* buffer, unsigned int size)
{
    Input* in = static_cast<Input*>(data);
    size_t n = in->len - in->pos;
    if (n > size) {
        n = size;
    }
    memcpy(buffer, in->data + in->pos, n);
    in->pos += n;
    return static_cast<int>(n);
}

} // namespace

long harness_decode(const unsigned char* data, size_t len, short* out, size_t cap)
{
    Input in { data, len, 0 };
    int channels, rate, samples;
    fallout::SoundDecoder* decoder = fallout::soundDecoderInit(read_input, &in, &channels, &rate, &samples);
    if (decoder == nullptr) {
        return -1;
    }
    size_t got = fallout::soundDecoderDecode(decoder, out, cap * sizeof(short)) / sizeof(short);
    fallout::soundDecoderFree(decoder);
    return static_cast<long>(got);
}

int main(int argc, char** argv)
{
    return harness_main(argc, argv);
}
