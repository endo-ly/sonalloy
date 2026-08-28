# External Audio Cross Synthesis

このReview Packageは、外部Audioを使う7つの固定条件をCLIから再生成します。assets/は入力、audio/は出力として分離しています。Full Cross Synthesisは48 kHzと96 kHzで生成します。

    python3 review/external-audio-cross-synthesis/scripts/generate_package.py

生成Scriptはreview/generate/common.pyのCLI実行・WAV測定・Block比較を利用します。入力WAVは録音素材ではなく、固定された数式から生成します。analysis/にはCLIのAnalysisとInspect、metrics.jsonには入力整列と固定長Bufferの測定結果を記録します。
