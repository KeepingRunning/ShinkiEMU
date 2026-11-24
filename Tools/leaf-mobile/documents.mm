#import <Foundation/Foundation.h>
#include <string>
std::string documents_directory() {
    @autoreleasepool {
        NSURL *url = [[[NSFileManager defaultManager] URLsForDirectory:NSDocumentDirectory inDomains:NSUserDomainMask] firstObject];
        return std::string(url.fileSystemRepresentation);
    }
}
