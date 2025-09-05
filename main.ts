import express from "express";
import cors from "cors";
import { clerkMiddleware } from "@clerk/express";
import http from "http";
import processRoutes from "./src/routes/process-routes";
import healthRoutes from "./src/routes/health-routes";
import apiKeyRoutes from "./src/routes/apiKey-routes";
import subscriptionRoutes from "./src/routes/subscription-routes";
import stripeRoutes from "./src/routes/stripe-routes";
import usageRoutes from "./src/routes/usage-routes";
import https from "https";
import fs from "fs";
import { apiLimiter } from "./src/middleware/rateLimiters";

const app = express();
const port = process.env.PORT || 9001;

app.use(cors({
	origin: '*',
	methods: ['GET', 'POST', 'PATCH', 'DELETE'],
	allowedHeaders: ['Origin', 'X-Requested-With', 'Content-Type', 'Accept', 'Authorization', 'X-API-Key']
}));

// Middleware to parse JSON request bodies
app.use(express.json());

app.use((err: any, req: any, res: any, next: any) => {
	console.error(err.stack);
	res.status(401).send("Unauthenticated!");
});

// Routes
app.use("/health", healthRoutes);
app.use("/process", processRoutes);

const apiRouter = express.Router();
apiRouter.use(apiLimiter);
apiRouter.use("/keys", apiKeyRoutes);
apiRouter.use("/subscription", subscriptionRoutes);
apiRouter.use("/stripe", stripeRoutes);
apiRouter.use("/usage", usageRoutes);

app.use("/api", apiRouter);

app.use((req, res) => {
	res.status(404).send("Not Found");
});

// TLS configuration
const keyPath = process.env.TLS_KEY_PATH;
const certPath = process.env.TLS_CERT_PATH;

let server;

if (keyPath && certPath && fs.existsSync(keyPath) && fs.existsSync(certPath)) {
	const options = {
		key: fs.readFileSync(keyPath),
		cert: fs.readFileSync(certPath),
	};
	server = https.createServer(options, app);
	console.log("TLS configuration loaded. Running in HTTPS mode.");
} else {
	server = http.createServer(app);
	if (keyPath || certPath) {
		if (!keyPath || !fs.existsSync(keyPath)) {
			console.error(`TLS key file not found at: ${keyPath}`);
		}
		if (!certPath || !fs.existsSync(certPath)) {
			console.error(`TLS certificate file not found at: ${certPath}`);
		}
		console.error("Proceeding without TLS.");
	} else {
		console.log("TLS environment variables not set. Running in HTTP mode.");
	}
}

server.listen(port, "0.0.0.0", () => {
	console.log(`Server is running over ${port - 1}`);
});
