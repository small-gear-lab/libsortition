/* Draw from a list of candidates with libsortition, from C (and C++: the same file builds as both).
 *
 *   cargo build --release            # builds target/release/libsortition.so (.dylib, .dll)
 *   make -C examples/c run
 *
 * The winners are the same as in every other binding and in the `sortition` command for the same
 * candidates and seed string: here "my seed" gives dave and bob. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "sortition.h"

int main(void) {
    /* 1. A seed string gives the commitment to publish before the draw, and the seed. */
    const char *seed_text = "my seed";
    uint64_t seed = 0;
    uint8_t commitment[32];
    sortition_seed_from_text((const uint8_t *)seed_text, strlen(seed_text), &seed, commitment);

    /* 2. Candidates are passed as one buffer plus the length of each one (any bytes will do). */
    const char *people[] = {"alice", "bob", "carol", "dave"};
    enum { COUNT = 4 };
    uint8_t data[64];
    size_t lens[COUNT], used = 0;
    for (int i = 0; i < COUNT; i++) {
        lens[i] = strlen(people[i]);
        memcpy(data + used, people[i], lens[i]);
        used += lens[i];
    }

    /* 3. Draw two candidates. The result does not depend on the order of the list. */
    uint32_t index[2];
    int32_t rc = sortition_items_draw(data, lens, COUNT, 2, &seed, index, NULL);
    if (rc != SORTITION_OK) {
        fprintf(stderr, "the draw failed (code %d)\n", (int)rc);
        return 1;
    }
    printf("%s %s\n", people[index[0]], people[index[1]]);

    /* 4. The fingerprint of the candidate set: publish it with the commitment before the draw. */
    uint8_t fingerprint[32];
    if (sortition_item_fingerprint(data, lens, COUNT, fingerprint) != SORTITION_OK) return 1;
    printf("commitment ");
    for (int i = 0; i < 4; i++) printf("%02x", commitment[i]);
    printf("..., fingerprint ");
    for (int i = 0; i < 4; i++) printf("%02x", fingerprint[i]);
    printf("... (libsortition %s)\n", sortition_version());
    return 0;
}
