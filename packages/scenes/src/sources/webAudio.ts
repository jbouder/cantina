import { FrameAnalyzer } from '../analyzer';
import { FFT_SIZE, type AudioFrame } from '../frame';
import type { FrameSource } from '../Stage';

/** How music-visualizer sets its analyzer, which the scenes are tuned to. */
export function configureAnalyser(analyser: AnalyserNode): AnalyserNode {
  analyser.fftSize = FFT_SIZE;
  analyser.smoothingTimeConstant = 0.8;
  analyser.minDecibels = -90;
  analyser.maxDecibels = -10;
  return analyser;
}

/** A frame source that reads a Web Audio analyzer, for an app with no engine. */
export class AnalyserFrames implements FrameSource {
  private readonly analyzer: FrameAnalyzer;

  constructor(private readonly analyser: AnalyserNode) {
    this.analyzer = new FrameAnalyzer(analyser.context.sampleRate);
  }

  readFrame(): AudioFrame {
    const frame = this.analyzer.frame;
    this.analyser.getByteFrequencyData(frame.freq as Uint8Array<ArrayBuffer>);
    this.analyser.getByteTimeDomainData(frame.wave as Uint8Array<ArrayBuffer>);
    return this.analyzer.update(this.analyser.context.currentTime);
  }

  reset(): void {
    this.analyzer.reset();
  }
}
