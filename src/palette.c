#include "palette.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void set_hex(float out[3], unsigned v) {
	out[0] = (v >> 16 & 255) / 255.0f;
	out[1] = (v >> 8 & 255) / 255.0f;
	out[2] = (v & 255) / 255.0f;
}

void palette_defaults(struct palette *p) {
	set_hex(p->sky, 0x0C0A12);
	set_hex(p->star, 0xE9D48F);
}

void palette_parse(const char *text, struct palette *p) {
	for (const char *line = text; *line;) {
		const char *end = strchr(line, '\n');
		size_t len = end ? (size_t)(end - line) : strlen(line);
		char buf[64];
		if (len < sizeof(buf)) {
			memcpy(buf, line, len);
			buf[len] = '\0';
			char *eq = strchr(buf, '=');
			if (eq) {
				*eq = '\0';
				char *key = buf, *val = eq + 1;
				while (*key == ' ') key++;
				for (char *e = key + strlen(key); e > key && e[-1] == ' '; ) *--e = '\0';
				while (*val == ' ' || *val == '#') val++;
				char *stop;
				unsigned long v = strtoul(val, &stop, 16);
				if (stop - val == 6 && !strcmp(key, "sky")) set_hex(p->sky, v);
				if (stop - val == 6 && !strcmp(key, "star")) set_hex(p->star, v);
			}
		}
		line = end ? end + 1 : line + len;
	}
}
