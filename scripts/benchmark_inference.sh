#!/bin/bash
# Benchmark script for comparing llama.cpp vs Magnitude inference engines
# Run with: ./scripts/benchmark_inference.sh

set -e

TELETYPE_ROOT="/Users/a.sorathiya/Documents/Ali/teletype"
cd "$TELETYPE_ROOT"

echo "=== Teletype Inference Engine Benchmark ==="
echo "Date: $(date)"
echo ""

# Check if Magnitude is running
echo "Checking Magnitude Desktop..."
if curl -s http://127.0.0.1:10100/inference/v1/models > /dev/null 2>&1; then
    echo "✓ Magnitude is running"
    MAGNITUDE_MODELS=$(curl -s http://127.0.0.1:10100/inference/v1/models | jq -r '.data[].id' | tr '\n' ' ')
    echo "  Available models: $MAGNITUDE_MODELS"
else
    echo "✗ Magnitude is not running on port 10100"
    echo "  Start Magnitude Desktop and download a model first"
    exit 1
fi

# Check if llama.cpp models are downloaded
echo ""
echo "Checking llama.cpp models..."
MODELS_DIR="$TELETYPE_ROOT/target/debug/models"
if [ -d "$MODELS_DIR" ]; then
    LLAMA_MODELS=$(ls -1 "$MODELS_DIR"/*.gguf 2>/dev/null | xargs -n1 basename | sed 's/\.gguf$//' | tr '\n' ' ')
    echo "  Downloaded models: $LLAMA_MODELS"
else
    echo "  No models directory found at $MODELS_DIR"
fi

# Test prompts for benchmarking
PROMPTS=(
    "hey john how are you doing today"
    "the quarterly report is due friday please review it and send feedback"
    "can you please schedule a meeting with the team for next tuesday at 2pm"
    "remember to buy milk eggs and bread on the way home"
    "the new feature implementation looks good but we need to add error handling"
)

echo ""
echo "=== Benchmark Configuration ==="
echo "Prompts: ${#PROMPTS[@]}"
echo "Iterations per prompt: 3"
echo ""

# Function to benchmark an endpoint
benchmark_endpoint() {
    local name=$1
    local url=$2
    local model=$3
    
    echo "--- Benchmarking $name ($model) ---"
    
    total_latency=0
    total_tokens=0
    success_count=0
    
    for prompt in "${PROMPTS[@]}"; do
        for i in {1..3}; do
            start=$(date +%s%3N)
            response=$(curl -s -X POST "$url/chat/completions" \
                -H "Content-Type: application/json" \
                -d "{
                    \"model\": \"$model\",
                    \"messages\": [{\"role\": \"user\", \"content\": \"$prompt\"}],
                    \"temperature\": 0,
                    \"max_tokens\": 300,
                    \"stream\": false
                }" 2>/dev/null || echo '{"error": "request failed"}')
            end=$(date +%s%3N)
            
            latency=$((end - start))
            
            # Extract token count from response
            tokens=$(echo "$response" | jq -r '.usage.total_tokens // 0' 2>/dev/null || echo 0)
            content=$(echo "$response" | jq -r '.choices[0].message.content // ""' 2>/dev/null || echo "")
            
            if [ "$tokens" -gt 0 ] && [ -n "$content" ] && [ "$content" != "null" ]; then
                echo "  ✓ $((latency))ms, $tokens tokens: ${content:0:50}..."
                total_latency=$((total_latency + latency))
                total_tokens=$((total_tokens + tokens))
                success_count=$((success_count + 1))
            else
                echo "  ✗ Failed: $(echo "$response" | jq -r '.error.message // .error // "unknown"' 2>/dev/null || echo "$response")"
            fi
        done
    done
    
    if [ $success_count -gt 0 ]; then
        avg_latency=$((total_latency / success_count))
        avg_tokens=$((total_tokens / success_count))
        tokens_per_sec=$((avg_tokens * 1000 / avg_latency))
        echo "  Average: ${avg_latency}ms latency, ${avg_tokens} tokens, ${tokens_per_sec} tok/s"
    else
        echo "  No successful requests"
    fi
    echo ""
}

# Benchmark llama.cpp (if model available)
if [ -n "$LLAMA_MODELS" ]; then
    # Use first available model
    FIRST_LLAMA=$(echo $LLAMA_MODELS | awk '{print $1}')
    # Need to start llama-server for this model
    echo "Starting llama-server for $FIRST_LLAMA..."
    # This would need the llama-server binary and model path
    # For now, skip if not already running
fi

# Benchmark Magnitude
FIRST_MAG=$(echo $MAGNITUDE_MODELS | awk '{print $1}')
if [ -n "$FIRST_MAG" ]; then
    benchmark_endpoint "Magnitude" "http://127.0.0.1:10100/inference/v1" "$FIRST_MAG"
else
    echo "No Magnitude models available for benchmarking"
fi

echo "=== Benchmark Complete ==="