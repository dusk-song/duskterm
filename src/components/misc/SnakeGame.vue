<script setup>
import { ref, onMounted, onUnmounted } from 'vue';

const gridSize = 12;
const tileCount = 20;
const CANVAS_SIZE = 240;

const canvasRef = ref(null);
let ctx = null;
let gameInterval = null;

const snake = ref([{ x: 10, y: 10 }]);
const food = ref({ x: 15, y: 15 });
const dx = ref(0);
const dy = ref(0);
const score = ref(0);
const isGameOver = ref(false);
const isPaused = ref(false);
const autoMode = ref(true); // Start in auto-play mode
let idleTimer = null;

// Input queue to handle rapid key presses
const inputQueue = [];

// Colors
const SNAKE_COLOR = '#00ff00';
const FOOD_COLOR = '#ff0000';
const BG_COLOR = '#000000';

const resetIdleTimer = () => {
  if (idleTimer) clearTimeout(idleTimer);
  idleTimer = setTimeout(() => {
    if (!autoMode.value) {
      autoMode.value = true;
      resetGame();
    }
  }, 5000); // 5 seconds idle
};

const initGame = () => {
    if (canvasRef.value) {
        ctx = canvasRef.value.getContext('2d');
        document.addEventListener('keydown', handleKeydown);
        gameInterval = setInterval(gameLoop, 100);
        resetIdleTimer();
    }
};

const resetGame = () => {
    snake.value = [{ x: 10, y: 10 }];
    food.value = { x: 15, y: 15 };
    dx.value = 0; // Wait for input
    dy.value = 0;
    inputQueue.length = 0; // Clear queue
    
    score.value = 0;
    isGameOver.value = false;
    isPaused.value = false;

    // In auto mode, start moving immediately
    if (autoMode.value) {
        dx.value = 1; 
        dy.value = 0;
    }
};

const handleKeydown = (e) => {
    resetIdleTimer();

    const isArrow = ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key);

    // If auto mode, switch to manual immediately
    if (autoMode.value) {
        if (isArrow) {
            autoMode.value = false;
            resetGame(); // Allow player to start fresh
        }
        return; 
    }

    if (isGameOver.value) {
        if (isArrow) {
            resetGame();
        }
        return;
    }

    // Determine the last planned direction (from queue or current motion)
    let lastDx = dx.value;
    let lastDy = dy.value;
    
    if (inputQueue.length > 0) {
        const lastMove = inputQueue[inputQueue.length - 1];
        lastDx = lastMove.dx;
        lastDy = lastMove.dy;
    }

    let targetDx = 0;
    let targetDy = 0;

    switch (e.key) {
        case 'ArrowUp': targetDx = 0; targetDy = -1; break;
        case 'ArrowDown': targetDx = 0; targetDy = 1; break;
        case 'ArrowLeft': targetDx = -1; targetDy = 0; break;
        case 'ArrowRight': targetDx = 1; targetDy = 0; break;
        default: return; // Ignore other keys
    }

    // Prevent 180 degree turns
    // If not moving yet (dx=0, dy=0), any direction is fine
    if (lastDx === 0 && lastDy === 0) {
        inputQueue.push({ dx: targetDx, dy: targetDy });
        return;
    }

    // Check opposition: strictly prevent reversing axis
    if (lastDx !== 0 && targetDx !== 0 && lastDx !== targetDx) return; // Ignore horizontal reverse
    if (lastDy !== 0 && targetDy !== 0 && lastDy !== targetDy) return; // Ignore vertical reverse
    
    // Also ignore if same direction (optimization)
    if (lastDx === targetDx && lastDy === targetDy) return;

    inputQueue.push({ dx: targetDx, dy: targetDy });
};

// AI for Auto Mode
const autoPlay = () => {
    const head = snake.value[0];
    const moves = [
        { dx: 0, dy: -1 }, // Up
        { dx: 0, dy: 1 },  // Down
        { dx: -1, dy: 0 }, // Left
        { dx: 1, dy: 0 }   // Right
    ];

    // 1. Filter physically possible moves (no reverse, no walls, no immediate body hit)
    const possibleMoves = moves.filter(m => {
        // Prevent reverse
        if (m.dx !== 0 && m.dx === -dx.value) return false;
        if (m.dy !== 0 && m.dy === -dy.value) return false;
        
        const nx = head.x + m.dx;
        const ny = head.y + m.dy;
        
        // Wall check
        if (nx < 0 || nx >= tileCount || ny < 0 || ny >= tileCount) return false;
        
        // Immediate body check
        for (let part of snake.value) {
            if (nx === part.x && ny === part.y) return false;
        }
        return true;
    });

    if (possibleMoves.length === 0) return; // No hope

    // 2. Flood Fill to check available space for each move
    // This looks ahead to prevent entering dead ends
    const getReachableSpace = (startX, startY) => {
        const visited = new Set();
        const queue = [{x: startX, y: startY}];
        visited.add(`${startX},${startY}`);
        let count = 0;
        // Limit search depth to optimize performance. 
        // We only need to know if we really have enough space for the snake's body.
        const limit = snake.value.length + 5; 

        // Snapshot obstacles (current snake body)
        const obstacles = new Set();
        for(let p of snake.value) obstacles.add(`${p.x},${p.y}`);
        
        while (queue.length > 0 && count < limit) {
             const {x, y} = queue.shift();
             count++;

             const neighbors = [
                {x: x+1, y}, {x: x-1, y}, {x, y: y+1}, {x, y: y-1}
             ];

             for (let n of neighbors) {
                 const key = `${n.x},${n.y}`;
                 if (n.x >= 0 && n.x < tileCount && n.y >= 0 && n.y < tileCount && 
                     !obstacles.has(key) && !visited.has(key)) {
                     visited.add(key);
                     queue.push(n);
                 }
             }
        }
        return count;
    };

    // Calculate scores
    possibleMoves.forEach(m => {
        m.space = getReachableSpace(head.x + m.dx, head.y + m.dy);
        m.dist = Math.abs((head.x + m.dx) - food.value.x) + Math.abs((head.y + m.dy) - food.value.y);
    });

    // 3. Sort moves
    // Priority 1: Must have enough space to fit the snake 
    // Priority 2: Closer to food
    possibleMoves.sort((a, b) => {
        const aSafe = a.space >= snake.value.length; // Use simple >= 
        const bSafe = b.space >= snake.value.length;

        if (aSafe && bSafe) {
            return a.dist - b.dist; // Both safe, pick food
        } else if (!aSafe && !bSafe) {
            return b.space - a.space; // Both trapped, pick max space
        } else {
            return aSafe ? -1 : 1; // Pick the safe one
        }
    });

    // Execute best move
    inputQueue.push(possibleMoves[0]);
};

const gameLoop = () => {
    if (isPaused.value || isGameOver.value) {
        if (autoMode.value && isGameOver.value) resetGame();
        return;
    }

    if (autoMode.value && inputQueue.length === 0) {
        autoPlay();
    }

    // Apply next move from queue if available
    if (inputQueue.length > 0) {
        const nextMove = inputQueue.shift();
        dx.value = nextMove.dx;
        dy.value = nextMove.dy;
    }

    // If game has not started yet (dx=0, dy=0), do not simulate physics
    if (dx.value === 0 && dy.value === 0) {
        draw();
        return;
    }

    const head = { x: snake.value[0].x + dx.value, y: snake.value[0].y + dy.value };

    // Wall collision
    if (head.x < 0 || head.x >= tileCount || head.y < 0 || head.y >= tileCount) {
        isGameOver.value = true;
        return;
    }

    // Self collision
    for (let i = 0; i < snake.value.length; i++) {
        if (head.x === snake.value[i].x && head.y === snake.value[i].y) {
            isGameOver.value = true;
            return;
        }
    }

    snake.value.unshift(head);

    // Eat food
    if (head.x === food.value.x && head.y === food.value.y) {
        score.value++;
        food.value = {
            x: Math.floor(Math.random() * tileCount),
            y: Math.floor(Math.random() * tileCount)
        };
    } else {
        snake.value.pop();
    }

    draw();
};

const draw = () => {
    if (!ctx) return;

    // Background
    ctx.fillStyle = BG_COLOR;
    ctx.fillRect(0, 0, CANVAS_SIZE, CANVAS_SIZE);

    // Food
    ctx.fillStyle = FOOD_COLOR;
    ctx.fillRect(food.value.x * gridSize, food.value.y * gridSize, gridSize - 2, gridSize - 2);

    // Snake
    ctx.fillStyle = SNAKE_COLOR;
    for (let i = 0; i < snake.value.length; i++) {
        ctx.fillRect(snake.value[i].x * gridSize, snake.value[i].y * gridSize, gridSize - 2, gridSize - 2);
    }
    
    // Grid lines (Retro effect)
    ctx.strokeStyle = '#111';
    for(let i=0; i<tileCount; i++) {
        ctx.beginPath();
        ctx.moveTo(i*gridSize, 0);
        ctx.lineTo(i*gridSize, CANVAS_SIZE);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(0, i*gridSize);
        ctx.lineTo(CANVAS_SIZE, i*gridSize);
        ctx.stroke();
    }
};

onMounted(() => {
    initGame();
});

onUnmounted(() => {
    clearInterval(gameInterval);
    if (idleTimer) clearTimeout(idleTimer);
    document.removeEventListener('keydown', handleKeydown);
});

</script>

<template>
    <div class="snake-container">
        <canvas ref="canvasRef" :width="CANVAS_SIZE" :height="CANVAS_SIZE" class="game-canvas"></canvas>
        <div class="game-overlay" v-if="!autoMode && (isGameOver || (score === 0 && dx === 0 && dy === 0))">
            <p v-if="isGameOver">游戏结束</p>
            <p v-else>按方向键开始</p>
            <p class="score">得分: {{ score }}</p>
        </div>
    </div>
</template>

<style scoped>
.snake-container {
    position: relative;
    width: 240px;
    height: 240px;
    border: 2px solid #333;
    border-radius: 4px;
    background: #000;
}

.game-canvas {
    display: block;
}

.game-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    color: #0f0;
    font-family: 'Press Start 2P', monospace; /* Fallback to monospace */
    font-weight: bold;
    text-shadow: 0 0 5px #0f0;
    pointer-events: none;
    background: rgba(0, 0, 0, 0.3);
}

.score {
    margin-top: 10px;
    font-size: 14px;
}

.blink {
    animation: blinker 1s linear infinite;
}

@keyframes blinker {
    50% {
        opacity: 0;
    }
}
</style>
