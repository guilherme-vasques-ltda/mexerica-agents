import Speech

struct OnDeviceSpeech {
    let supportsOnDeviceRecognition: Bool
    let isAvailable: Bool

    var canStart: Bool { supportsOnDeviceRecognition && isAvailable }

    func makeRequest() -> SFSpeechAudioBufferRecognitionRequest? {
        guard canStart else { return nil }
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.requiresOnDeviceRecognition = true
        request.shouldReportPartialResults = true
        return request
    }
}
