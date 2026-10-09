#include "sonalloy.h"

int main() {
    return sonalloy_c_api_version() == SONALLOY_C_API_VERSION ? 0 : 1;
}
