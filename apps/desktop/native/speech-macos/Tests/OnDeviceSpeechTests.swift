import Speech
import Testing
@testable import codync_speech

@Test func noRequestWithoutLocalSupportOrAvailability() {
    let remoteOnly = OnDeviceSpeech(supportsOnDeviceRecognition: false, isAvailable: true)
    let temporarilyUnavailable = OnDeviceSpeech(supportsOnDeviceRecognition: true, isAvailable: false)
    #expect(!remoteOnly.canStart)
    #expect(remoteOnly.makeRequest() == nil)
    #expect(!temporarilyUnavailable.canStart)
    #expect(temporarilyUnavailable.makeRequest() == nil)
}

@Test func acceptedRequestRequiresOnDeviceRecognition() throws {
    let local = OnDeviceSpeech(supportsOnDeviceRecognition: true, isAvailable: true)
    #expect(local.canStart)
    let request = try #require(local.makeRequest())
    #expect(request.requiresOnDeviceRecognition)
    #expect(request.shouldReportPartialResults)
}
