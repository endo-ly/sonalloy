#include "sonalloy.h"

int main() {
    return sonalloy_c_api_version() == 1 ? 0 : 1;
}
